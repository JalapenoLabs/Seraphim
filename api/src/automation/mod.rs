//! The automation rules engine: the rule shape (triggers, condition group,
//! action) and the pure matcher that decides whether an issue event satisfies a
//! rule. Storage (the `automation_rules` row) and firing (querying rules, running
//! the action) live elsewhere; this module is deliberately free of I/O so the
//! matching logic is unit-tested in isolation.

use serde::{Deserialize, Serialize};

/// What kind of event fires a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// The issue was just opened.
    Created,
    /// The issue changed (edited, labeled, reopened, ...).
    Updated,
    /// A comment was posted on the issue.
    Comment,
}

/// How a group's conditions combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Combinator {
    And,
    Or,
}

/// The issue attribute a condition tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// The issue's labels (a list).
    Labels,
    /// The issue author's login.
    Author,
    /// The repository full name (`owner/repo`).
    Repo,
    Title,
    Body,
    /// The triggering comment's body (empty unless this is a comment event).
    Comment,
    /// The triggering comment's author login.
    CommentAuthor,
    /// The issue state (`open` / `closed`).
    State,
}

/// How a condition compares the field to its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    /// Equals one of the values (case-insensitive).
    Exactly,
    /// Contains one of the values as a substring (case-insensitive).
    Contains,
    /// For a list field: shares at least one value. For a scalar: same as `exactly`.
    HasOneOf,
    /// The field is empty.
    IsEmpty,
    /// The field is non-empty.
    IsNotEmpty,
}

/// One condition: a field tested by an operator against zero or more values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub field: Field,
    pub operator: Operator,
    #[serde(default)]
    pub values: Vec<String>,
}

/// A group of conditions combined with AND / OR. An empty group always matches
/// (the trigger and source gates still apply), so a rule can fire on every event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleGroup {
    pub combinator: Combinator,
    #[serde(default)]
    pub conditions: Vec<Condition>,
}

/// Where in To Do a matched card lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuePosition {
    Top,
    Bottom,
}

/// What a matched rule does. Tagged so new actions can be added later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleAction {
    /// Move the card to To Do, at the top or bottom of the queue.
    MoveToTodo { position: QueuePosition },
}

/// The event being matched, gathered from a webhook payload. Scalar fields that
/// don't apply to a given event (e.g. the comment on a non-comment event) are
/// empty strings.
#[derive(Debug, Clone, Copy)]
pub struct RuleContext<'a> {
    pub trigger: Trigger,
    pub repo: &'a str,
    pub author: &'a str,
    pub labels: &'a [String],
    pub title: &'a str,
    pub body: &'a str,
    pub state: &'a str,
    pub comment: &'a str,
    pub comment_author: &'a str,
}

impl RuleGroup {
    /// Whether this group matches the event.
    pub fn matches(&self, ctx: &RuleContext) -> bool {
        if self.conditions.is_empty() {
            return true;
        }
        match self.combinator {
            Combinator::And => self.conditions.iter().all(|c| c.matches(ctx)),
            Combinator::Or => self.conditions.iter().any(|c| c.matches(ctx)),
        }
    }
}

impl Condition {
    fn matches(&self, ctx: &RuleContext) -> bool {
        match self.field {
            Field::Labels => self.eval_list(ctx.labels),
            Field::Author => self.eval_scalar(ctx.author),
            Field::Repo => self.eval_scalar(ctx.repo),
            Field::Title => self.eval_scalar(ctx.title),
            Field::Body => self.eval_scalar(ctx.body),
            Field::Comment => self.eval_scalar(ctx.comment),
            Field::CommentAuthor => self.eval_scalar(ctx.comment_author),
            Field::State => self.eval_scalar(ctx.state),
        }
    }

    /// Non-empty values, trimmed and lowercased, for case-insensitive matching.
    fn needles(&self) -> impl Iterator<Item = String> + '_ {
        self.values
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
    }

    fn eval_scalar(&self, value: &str) -> bool {
        let value = value.trim().to_ascii_lowercase();
        match self.operator {
            Operator::IsEmpty => value.is_empty(),
            Operator::IsNotEmpty => !value.is_empty(),
            Operator::Exactly | Operator::HasOneOf => self.needles().any(|needle| needle == value),
            Operator::Contains => self.needles().any(|needle| value.contains(&needle)),
        }
    }

    fn eval_list(&self, items: &[String]) -> bool {
        let present: Vec<String> = items
            .iter()
            .map(|item| item.trim().to_ascii_lowercase())
            .filter(|item| !item.is_empty())
            .collect();
        match self.operator {
            Operator::IsEmpty => present.is_empty(),
            Operator::IsNotEmpty => !present.is_empty(),
            Operator::Exactly | Operator::HasOneOf => self
                .needles()
                .any(|needle| present.iter().any(|item| item == &needle)),
            Operator::Contains => self
                .needles()
                .any(|needle| present.iter().any(|item| item.contains(&needle))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(trigger: Trigger, labels: &[String]) -> RuleContext<'_> {
        RuleContext {
            trigger,
            repo: "navarrotech/seraphim",
            author: "navarrotech",
            labels,
            title: "Fix the thing",
            body: "It is broken",
            state: "open",
            comment: "",
            comment_author: "",
        }
    }

    fn condition(field: Field, operator: Operator, values: &[&str]) -> Condition {
        Condition {
            field,
            operator,
            values: values.iter().map(|&value| value.to_string()).collect(),
        }
    }

    #[test]
    fn empty_group_always_matches() {
        let group = RuleGroup {
            combinator: Combinator::And,
            conditions: vec![],
        };
        assert!(group.matches(&ctx(Trigger::Created, &[])));
    }

    #[test]
    fn labels_has_one_of_and_author_exactly_with_and() {
        // Mirrors the issue's example: labels has one of {automation,bug} AND
        // author is exactly navarrotech.
        let labels = vec!["bug".to_string(), "ux".to_string()];
        let group = RuleGroup {
            combinator: Combinator::And,
            conditions: vec![
                condition(Field::Labels, Operator::HasOneOf, &["automation", "bug"]),
                condition(Field::Author, Operator::Exactly, &["navarrotech"]),
            ],
        };
        assert!(group.matches(&ctx(Trigger::Created, &labels)));

        // Wrong author fails the AND.
        let mut wrong = ctx(Trigger::Created, &labels);
        wrong.author = "someone-else";
        assert!(!group.matches(&wrong));

        // A label outside the set fails has-one-of.
        let other = vec!["docs".to_string()];
        assert!(!group.matches(&ctx(Trigger::Created, &other)));
    }

    #[test]
    fn or_combinator_needs_only_one() {
        let labels: Vec<String> = vec![];
        let group = RuleGroup {
            combinator: Combinator::Or,
            conditions: vec![
                condition(Field::Author, Operator::Exactly, &["nobody"]),
                condition(Field::Title, Operator::Contains, &["thing"]),
            ],
        };
        assert!(group.matches(&ctx(Trigger::Updated, &labels)));
    }

    #[test]
    fn comment_contains_trigger_phrase() {
        // The "Jarvis, can you take this on?" flow.
        let mut context = ctx(Trigger::Comment, &[]);
        context.comment = "Hey Jarvis, can you take this on? thanks";
        let group = RuleGroup {
            combinator: Combinator::And,
            conditions: vec![condition(
                Field::Comment,
                Operator::Contains,
                &["jarvis, can you take this on?"],
            )],
        };
        assert!(group.matches(&context));
    }

    #[test]
    fn is_empty_and_is_not_empty() {
        let labels: Vec<String> = vec![];
        let empty_labels = RuleGroup {
            combinator: Combinator::And,
            conditions: vec![condition(Field::Labels, Operator::IsEmpty, &[])],
        };
        assert!(empty_labels.matches(&ctx(Trigger::Created, &labels)));

        let has_labels = vec!["bug".to_string()];
        assert!(!empty_labels.matches(&ctx(Trigger::Created, &has_labels)));

        let not_empty_body = RuleGroup {
            combinator: Combinator::And,
            conditions: vec![condition(Field::Body, Operator::IsNotEmpty, &[])],
        };
        assert!(not_empty_body.matches(&ctx(Trigger::Created, &labels)));
    }

    #[test]
    fn action_round_trips_through_json() {
        let action = RuleAction::MoveToTodo {
            position: QueuePosition::Bottom,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, r#"{"type":"move_to_todo","position":"bottom"}"#);
        let parsed: RuleAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, action);
    }
}
