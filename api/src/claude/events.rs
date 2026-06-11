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
//! - `{"type":"rate_limit_event","rate_limit_info":{...},"session_id":...}`
//!
//! With `--include-partial-messages` Claude Code also emits a firehose of
//! `stream_event` lines (the raw Messages-API streaming protocol). We mine only
//! the live token usage from `message_start` / `message_delta` and drop the rest,
//! so the partial stream feeds the live counter without bloating the event log.

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
    /// A subscription rate-limit notice. The structured `rate_limit_info` rides
    /// along in the event payload (`raw`) for the UI to render; we only classify
    /// the line here so it can be styled instead of dumped as raw JSON.
    RateLimit,
    /// Live token usage from a partial-message stream event (opted in with
    /// `--include-partial-messages`). `message_start` carries the prompt cost
    /// (`input_tokens` + cache fields); `message_delta` carries the live, rising
    /// `output_tokens` for the current message. Used only to tick the stats
    /// gauges, never persisted to the event log. `input_tokens.is_some()` marks a
    /// `message_start` (a new assistant message), which restarts the per-message
    /// output count.
    Usage {
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cache_read_input_tokens: Option<i64>,
        cache_creation_input_tokens: Option<i64>,
    },
    /// Any other line, preserved as-is.
    Other,
}

/// Accumulates the turn-level live token usage from the partial-message stream.
///
/// `message_delta.usage.output_tokens` is cumulative *per assistant message* and
/// restarts at each `message_start` (every tool round-trip is a new message), so
/// a turn total must sum each completed message's output and add the current
/// message's live value. Input/cache recur per round-trip, so the context size is
/// the latest message's prompt, not a sum.
#[derive(Debug, Default, Clone, Copy)]
pub struct UsageTracker {
    /// Finalized output of the assistant messages completed so far this turn.
    output_base: i64,
    /// The current message's live (monotonically rising) output count.
    current_output: i64,
    /// The current message's prompt size (input + cache reads + cache creation).
    context: i64,
}

impl UsageTracker {
    /// Folds one [`AgentEventKind::Usage`] event in, given its four token fields.
    /// `input_tokens.is_some()` marks a `message_start` (a new assistant message).
    pub fn apply(
        &mut self,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cache_read_input_tokens: Option<i64>,
        cache_creation_input_tokens: Option<i64>,
    ) {
        if input_tokens.is_some() {
            // A new assistant message: bank the previous message's output and
            // restart the per-message count and the prompt size.
            self.output_base += self.current_output;
            self.current_output = output_tokens.unwrap_or(0);
            self.context = input_tokens.unwrap_or(0)
                + cache_read_input_tokens.unwrap_or(0)
                + cache_creation_input_tokens.unwrap_or(0);
        } else if let Some(output) = output_tokens {
            // A `message_delta`: the live output count for the current message.
            self.current_output = output;
        }
    }

    /// Turn-cumulative output tokens generated so far.
    pub fn output_tokens(&self) -> i64 {
        self.output_base + self.current_output
    }

    /// The current assistant message's prompt size, for the context gauge.
    pub fn context_tokens(&self) -> i64 {
        self.context
    }
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
            AgentEventKind::RateLimit => "rate_limit",
            AgentEventKind::Usage { .. } => "usage",
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
        // A periodic usage notice (`rate_limit_info` payload). Classify it so the
        // UI can render it cleanly rather than dumping the raw JSON line.
        Some("rate_limit_event") => vec![AgentEvent {
            kind: AgentEventKind::RateLimit,
            session_id,
            raw: value,
        }],
        // Partial-message stream events (`--include-partial-messages`): a firehose
        // wrapped as `stream_event`. We extract only the live token usage from
        // `message_start` / `message_delta` and drop every other partial
        // (`content_block_delta`, `ping`, `message_stop`, ...) so they never reach
        // the event log or the live feed.
        Some("stream_event") => {
            let inner = value.get("event").cloned().unwrap_or(Value::Null);
            parse_stream_event(&inner, session_id, value)
        }
        // Defensive: some Claude Code versions may surface the raw partial type at
        // the top level rather than wrapped in `stream_event`.
        Some("message_start" | "message_delta") => {
            parse_stream_event(&value, session_id, value.clone())
        }
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

/// Extracts the live token usage from a partial-message stream event, dropping
/// everything else. `message_start` carries the prompt cost on `message.usage`;
/// `message_delta` carries the rising output count on `usage`.
fn parse_stream_event(inner: &Value, session_id: Option<String>, raw: Value) -> Vec<AgentEvent> {
    let usage = match inner.get("type").and_then(Value::as_str) {
        Some("message_start") => inner.pointer("/message/usage"),
        Some("message_delta") => inner.get("usage"),
        // content_block_delta, content_block_start/stop, message_stop, ping, ...
        _ => None,
    };
    usage_event(usage, session_id, raw)
        .map(|event| vec![event])
        .unwrap_or_default()
}

/// Builds a [`AgentEventKind::Usage`] event from a Messages-API `usage` object,
/// or `None` when there is nothing countable to report.
fn usage_event(
    usage: Option<&Value>,
    session_id: Option<String>,
    raw: Value,
) -> Option<AgentEvent> {
    let usage = usage?;
    let field = |key: &str| usage.get(key).and_then(Value::as_i64);
    let input_tokens = field("input_tokens");
    let output_tokens = field("output_tokens");
    // A usage block with neither an input nor an output count carries no signal.
    if input_tokens.is_none() && output_tokens.is_none() {
        return None;
    }
    Some(AgentEvent {
        kind: AgentEventKind::Usage {
            input_tokens,
            output_tokens,
            cache_read_input_tokens: field("cache_read_input_tokens"),
            cache_creation_input_tokens: field("cache_creation_input_tokens"),
        },
        session_id,
        raw,
    })
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
    fn message_start_partial_yields_input_and_cache_usage() {
        // The prompt-cost snapshot at the start of an assistant message.
        let line = r#"{"type":"stream_event","session_id":"s1","event":{"type":"message_start","message":{"usage":{"input_tokens":1325,"cache_read_input_tokens":40000,"cache_creation_input_tokens":12,"output_tokens":3}}}}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].type_label(), "usage");
        assert_eq!(
            events[0].kind,
            AgentEventKind::Usage {
                input_tokens: Some(1325),
                output_tokens: Some(3),
                cache_read_input_tokens: Some(40000),
                cache_creation_input_tokens: Some(12),
            }
        );
    }

    #[test]
    fn message_delta_partial_yields_live_output_only() {
        // The live, rising output count mid-generation (the example from the issue).
        let line = r#"{"type":"stream_event","session_id":"s1","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":128}}}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].kind,
            AgentEventKind::Usage {
                input_tokens: None,
                output_tokens: Some(128),
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
            }
        );
    }

    #[test]
    fn bare_top_level_partial_is_also_parsed() {
        // Defensive path: a partial surfaced without the `stream_event` wrapper.
        let line = r#"{"type":"message_delta","usage":{"output_tokens":7}}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::Usage { output_tokens, .. } => assert_eq!(*output_tokens, Some(7)),
            other => panic!("expected usage, got {other:?}"),
        }
    }

    #[test]
    fn usage_tracker_accumulates_output_across_message_boundaries() {
        let mut tracker = UsageTracker::default();

        // First assistant message: prompt cost, then output rises to 40.
        tracker.apply(Some(1000), Some(2), Some(30000), Some(0));
        assert_eq!(tracker.context_tokens(), 31000);
        tracker.apply(None, Some(12), None, None);
        tracker.apply(None, Some(40), None, None);
        assert_eq!(tracker.output_tokens(), 40);

        // A tool round-trip starts a new message: output resets per message, but
        // the turn total keeps the prior 40 and the context reflects the new
        // (larger, re-sent) prompt.
        tracker.apply(Some(1500), Some(1), Some(32000), Some(8));
        assert_eq!(tracker.context_tokens(), 33508);
        assert_eq!(tracker.output_tokens(), 41); // 40 banked + 1 initial
        tracker.apply(None, Some(25), None, None);
        assert_eq!(tracker.output_tokens(), 65); // 40 banked + 25 live
    }

    #[test]
    fn other_partial_events_are_dropped() {
        // The firehose (content deltas, pings, stops) must not reach the event log.
        for line in [
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":0}}"#,
            r#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
            r#"{"type":"stream_event","event":{"type":"ping"}}"#,
            // A message_delta with no usage block carries no countable signal.
            r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}"#,
        ] {
            assert!(parse_line(line).is_empty(), "should drop: {line}");
        }
    }

    #[test]
    fn non_json_is_preserved_as_other() {
        let events = parse_line("warning: something happened");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AgentEventKind::Other);
    }

    #[test]
    fn classifies_rate_limit_event_and_keeps_its_payload() {
        let line = r#"{"type":"rate_limit_event","rate_limit_info":{"rateLimitType":"five_hour","status":"allowed","resetsAt":1781142000},"session_id":"s9"}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AgentEventKind::RateLimit);
        assert_eq!(events[0].type_label(), "rate_limit");
        // The structured info rides along in the payload for the UI to render.
        assert_eq!(
            events[0].raw.pointer("/rate_limit_info/rateLimitType"),
            Some(&Value::String("five_hour".to_string()))
        );
    }
}
