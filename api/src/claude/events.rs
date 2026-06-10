//! Parsing of Claude Code's `--output-format stream-json` output.
//!
//! Each line Claude emits is a standalone JSON object tagged by a `type` field.
//! We normalize the handful of shapes we care about into [`AgentEvent`]s that the
//! orchestrator persists and streams to the UI. Unknown shapes are preserved
//! verbatim as [`AgentEventKind::Other`] rather than dropped, so the parser
//! degrades gracefully if Claude Code's schema shifts between versions.
//!
//! The documented top-level shapes are:
//! - `{"type":"system","subtype":"init","session_id":...,"model":...}`
//! - `{"type":"assistant","message":{content:[...]},"session_id":...}`
//! - `{"type":"user","message":{content:[{type:"tool_result",...}]}}`
//! - `{"type":"result","subtype":"success","result":...,"session_id":...,"total_cost_usd":...}`

use serde_json::Value;

/// A normalized event distilled from one stream-json line.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentEvent {
    pub kind: AgentEventKind,
    /// Session id, present on `system`/`init` and `result` lines.
    pub session_id: Option<String>,
    /// The original JSON line, retained for the persisted event payload.
    pub raw: Value,
}

/// The meaningful flavors of stream-json output.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEventKind {
    /// Session initialization; carries the model name.
    Init { model: Option<String> },
    /// The agent's extended-thinking / reasoning.
    Thinking { text: String },
    /// A chunk of assistant prose.
    AssistantText { text: String },
    /// The agent invoking a tool.
    ToolUse { name: String, input: Value },
    /// The result of a tool invocation, flattened to text.
    ToolResult { text: String },
    /// The terminal line of a turn, with cost and final text.
    Result {
        total_cost_usd: Option<f64>,
        result_text: Option<String>,
        is_error: bool,
    },
    /// Any other line, preserved as-is.
    Other,
}

impl AgentEvent {
    /// The short label stored in the `events.type` column.
    pub fn type_label(&self) -> &'static str {
        match self.kind {
            AgentEventKind::Init { .. } => "system",
            AgentEventKind::Thinking { .. } => "thinking",
            AgentEventKind::AssistantText { .. } => "assistant_text",
            AgentEventKind::ToolUse { .. } => "tool_use",
            AgentEventKind::ToolResult { .. } => "tool_result",
            AgentEventKind::Result { .. } => "result",
            AgentEventKind::Other => "other",
        }
    }
}

/// Parses one stream-json line into zero or more [`AgentEvent`]s.
///
/// A single `assistant` line can carry several content blocks (text plus tool
/// calls), so this returns a `Vec`. Blank lines and unparseable lines yield an
/// empty vec and a single `Other` event respectively.
pub fn parse_line(line: &str) -> Vec<AgentEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        // Non-JSON output (e.g. a stray log line) is surfaced, not swallowed.
        return vec![AgentEvent {
            kind: AgentEventKind::Other,
            session_id: None,
            raw: Value::String(trimmed.to_string()),
        }];
    };

    let session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string);

    match value.get("type").and_then(Value::as_str) {
        // Only the `init` system line is meaningful here. Claude Code also emits a
        // stream of other `system` subtypes (`thinking_tokens`, `task_started`,
        // `task_notification`, `status`, `compact_boundary`, ...) as live
        // telemetry; surfacing those would flood the activity log, so drop them.
        Some("system") => match value.get("subtype").and_then(Value::as_str) {
            Some("init") => vec![AgentEvent {
                kind: AgentEventKind::Init {
                    model: value
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
                session_id,
                raw: value,
            }],
            _ => Vec::new(),
        },
        Some("assistant") => parse_assistant(&value, session_id.as_deref(), value.clone()),
        Some("user") => parse_user(&value, session_id.as_deref()),
        Some("result") => vec![AgentEvent {
            kind: AgentEventKind::Result {
                total_cost_usd: value.get("total_cost_usd").and_then(Value::as_f64),
                result_text: value
                    .get("result")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                // Honor the explicit `is_error` boolean first; fall back to the
                // `subtype` for older shapes. (A "Not logged in" result reports
                // is_error=true even though subtype is "success".)
                is_error: value
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or_else(|| {
                        value
                            .get("subtype")
                            .and_then(Value::as_str)
                            .is_some_and(|subtype| subtype != "success")
                    }),
            },
            session_id,
            raw: value,
        }],
        _ => vec![AgentEvent {
            kind: AgentEventKind::Other,
            session_id,
            raw: value,
        }],
    }
}

/// Expands an `assistant` line's content blocks into text and tool-use events.
fn parse_assistant(value: &Value, session_id: Option<&str>, raw: Value) -> Vec<AgentEvent> {
    let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) else {
        return vec![AgentEvent {
            kind: AgentEventKind::Other,
            session_id: session_id.map(str::to_string),
            raw,
        }];
    };

    let mut events = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("thinking") => {
                // Extended-thinking blocks carry their text under `thinking`.
                // Skip empty ones (e.g. when the model omits thinking text).
                if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        events.push(AgentEvent {
                            kind: AgentEventKind::Thinking {
                                text: text.to_string(),
                            },
                            session_id: session_id.map(str::to_string),
                            raw: block.clone(),
                        });
                    }
                }
            }
            Some("redacted_thinking") => {
                events.push(AgentEvent {
                    kind: AgentEventKind::Thinking {
                        text: "[redacted thinking]".to_string(),
                    },
                    session_id: session_id.map(str::to_string),
                    raw: block.clone(),
                });
            }
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    events.push(AgentEvent {
                        kind: AgentEventKind::AssistantText {
                            text: text.to_string(),
                        },
                        session_id: session_id.map(str::to_string),
                        raw: block.clone(),
                    });
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                events.push(AgentEvent {
                    kind: AgentEventKind::ToolUse { name, input },
                    session_id: session_id.map(str::to_string),
                    raw: block.clone(),
                });
            }
            _ => {}
        }
    }
    events
}

/// Extracts tool results from a `user` line, flattening their content to text.
fn parse_user(value: &Value, session_id: Option<&str>) -> Vec<AgentEvent> {
    let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut events = Vec::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("tool_result") {
            events.push(AgentEvent {
                kind: AgentEventKind::ToolResult {
                    text: flatten_tool_result(block.get("content")),
                },
                session_id: session_id.map(str::to_string),
                raw: block.clone(),
            });
        }
    }
    events
}

/// Tool-result content may be a bare string or an array of content blocks.
fn flatten_tool_result(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_lines_yield_nothing() {
        assert!(parse_line("   ").is_empty());
        assert!(parse_line("").is_empty());
    }

    #[test]
    fn parses_init_line() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc-123","model":"claude-opus-4-8"}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id.as_deref(), Some("abc-123"));
        assert_eq!(
            events[0].kind,
            AgentEventKind::Init {
                model: Some("claude-opus-4-8".to_string())
            }
        );
    }

    #[test]
    fn non_init_system_telemetry_is_dropped() {
        // Streaming telemetry (e.g. thinking_tokens) must not flood the log.
        let line = r#"{"type":"system","subtype":"thinking_tokens","session_id":"s1","estimated_tokens":1100}"#;
        assert!(parse_line(line).is_empty());
    }

    #[test]
    fn assistant_line_splits_text_and_tool_use() {
        let line = r#"{"type":"assistant","session_id":"s1","message":{"content":[
            {"type":"text","text":"Looking into it"},
            {"type":"tool_use","name":"Read","input":{"path":"foo.rs"}}
        ]}}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].kind,
            AgentEventKind::AssistantText {
                text: "Looking into it".to_string()
            }
        );
        match &events[1].kind {
            AgentEventKind::ToolUse { name, input } => {
                assert_eq!(name, "Read");
                assert_eq!(input.get("path").unwrap(), "foo.rs");
            }
            other => panic!("expected tool_use, got {other:?}"),
        }
    }

    #[test]
    fn assistant_line_emits_thinking_then_text() {
        let line = r#"{"type":"assistant","session_id":"s1","message":{"content":[
            {"type":"thinking","thinking":"Let me check the config first."},
            {"type":"thinking","thinking":"   "},
            {"type":"text","text":"Done."}
        ]}}"#;
        let events = parse_line(line);
        // Empty thinking is skipped, so: one thinking + one text.
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].kind,
            AgentEventKind::Thinking {
                text: "Let me check the config first.".to_string()
            }
        );
        assert_eq!(events[0].type_label(), "thinking");
        assert_eq!(
            events[1].kind,
            AgentEventKind::AssistantText {
                text: "Done.".to_string()
            }
        );
    }

    #[test]
    fn user_line_flattens_tool_result_array() {
        let line = r#"{"type":"user","message":{"content":[
            {"type":"tool_result","content":[{"type":"text","text":"line one"},{"type":"text","text":"line two"}]}
        ]}}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].kind,
            AgentEventKind::ToolResult {
                text: "line one\nline two".to_string()
            }
        );
    }

    #[test]
    fn parses_result_line_with_cost() {
        let line = r#"{"type":"result","subtype":"success","result":"done","session_id":"s9","total_cost_usd":0.0123}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id.as_deref(), Some("s9"));
        match &events[0].kind {
            AgentEventKind::Result {
                total_cost_usd,
                result_text,
                is_error,
            } => {
                assert_eq!(*total_cost_usd, Some(0.0123));
                assert_eq!(result_text.as_deref(), Some("done"));
                assert!(!is_error);
            }
            other => panic!("expected result, got {other:?}"),
        }
    }

    #[test]
    fn result_error_subtype_is_flagged() {
        let line = r#"{"type":"result","subtype":"error_max_turns","session_id":"s9"}"#;
        let events = parse_line(line);
        match &events[0].kind {
            AgentEventKind::Result { is_error, .. } => assert!(is_error),
            other => panic!("expected result, got {other:?}"),
        }
    }

    #[test]
    fn explicit_is_error_overrides_success_subtype() {
        // The "Not logged in" case: subtype success but is_error true.
        let line = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in","session_id":"s9"}"#;
        let events = parse_line(line);
        match &events[0].kind {
            AgentEventKind::Result {
                is_error,
                result_text,
                ..
            } => {
                assert!(is_error);
                assert_eq!(result_text.as_deref(), Some("Not logged in"));
            }
            other => panic!("expected result, got {other:?}"),
        }
    }

    #[test]
    fn non_json_is_preserved_as_other() {
        let events = parse_line("warning: something happened");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AgentEventKind::Other);
    }
}
