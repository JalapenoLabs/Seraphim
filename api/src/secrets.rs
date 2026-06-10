//! Masking and scrubbing of secret values.
//!
//! Two related jobs live here:
//!
//! - [`mask`] turns a raw secret into a human-identifiable preview, e.g.
//!   `sk_live_123456789` becomes `sk_live_*****6789`. It is used both for the
//!   settings page (so an operator can recognize a stored token without it being
//!   fully revealed) and as the replacement text when scrubbing.
//! - [`Scrubber`] removes a known set of secret values from arbitrary text and
//!   JSON, so a secret the agent happens to echo never reaches the database, the
//!   live event stream, or the logs.
//!
//! Both are pure and unit-tested; the orchestrator builds a [`Scrubber`] from the
//! configured secrets once per turn and runs every event through it.

use std::collections::HashSet;

use serde_json::Value;

/// Recognized secret prefixes whose identifying lead-in is kept when masking.
///
/// Keeping the prefix (plus the last few characters) lets an operator tell two
/// tokens apart at a glance without exposing the secret itself.
const KNOWN_PREFIXES: &[&str] = &[
    "sk-ant-",
    "github_pat_",
    "ghp_",
    "gho_",
    "ghs_",
    "sk_live_",
    "sk_test_",
];

/// How many trailing characters stay visible when a known prefix is recognized.
const REVEALED_TAIL: usize = 4;

/// Shortest secret length we will scrub from output.
///
/// Scrubbing replaces every occurrence of a secret, so a very short value (say
/// `"1"`) would corrupt unrelated text. Real tokens are long; we refuse to scrub
/// anything shorter than this to stay safe. Masking, by contrast, applies to any
/// length.
const MIN_SCRUB_LEN: usize = 4;

/// Masks a secret into an identifiable preview, preserving its length.
///
/// A value beginning with a [`KNOWN_PREFIXES`] entry keeps that prefix and its
/// last [`REVEALED_TAIL`] characters, with the middle replaced by `*`. Any other
/// value is fully masked. An empty input returns an empty string.
///
/// # Examples
/// ```ignore
/// assert_eq!(mask("sk_live_123456789"), "sk_live_*****6789");
/// assert_eq!(mask("12345"), "*****");
/// ```
pub fn mask(secret: &str) -> String {
    let total = secret.chars().count();
    if total == 0 {
        return String::new();
    }

    let Some(prefix) = KNOWN_PREFIXES
        .iter()
        .copied()
        .find(|prefix| secret.starts_with(prefix))
    else {
        // No recognized prefix: fully mask, honoring length.
        return "*".repeat(total);
    };

    let prefix_len = prefix.chars().count();
    // Only reveal the tail when doing so still hides a meaningful middle section;
    // otherwise keep just the prefix so we never expose most of a short secret.
    if total > prefix_len + REVEALED_TAIL {
        let masked = total - prefix_len - REVEALED_TAIL;
        let tail: String = secret.chars().skip(total - REVEALED_TAIL).collect();
        format!("{prefix}{}{tail}", "*".repeat(masked))
    } else {
        format!("{prefix}{}", "*".repeat(total - prefix_len))
    }
}

/// Replaces known secret values with their masked form wherever they appear.
///
/// Built once from the configured secrets, then applied to every persisted or
/// streamed event. Replacements run longest-secret-first so that one secret
/// containing another is masked before its shorter substring.
#[derive(Debug, Clone, Default)]
pub struct Scrubber {
    /// `(secret, mask)` pairs, sorted by descending secret length.
    replacements: Vec<(String, String)>,
}

impl Scrubber {
    /// Builds a scrubber from raw secret values, ignoring empties, duplicates,
    /// and values too short to scrub safely (see [`MIN_SCRUB_LEN`]).
    pub fn new<I: IntoIterator<Item = String>>(secrets: I) -> Self {
        let mut seen = HashSet::new();
        let mut replacements: Vec<(String, String)> = secrets
            .into_iter()
            .filter(|secret| secret.chars().count() >= MIN_SCRUB_LEN)
            .filter(|secret| seen.insert(secret.clone()))
            .map(|secret| {
                let masked = mask(&secret);
                (secret, masked)
            })
            .collect();
        // Longest first so a secret that contains another is replaced before it.
        replacements.sort_by(|left, right| right.0.len().cmp(&left.0.len()));
        Self { replacements }
    }

    /// Returns `text` with every known secret replaced by its masked form.
    pub fn scrub_text(&self, text: &str) -> String {
        let mut scrubbed = text.to_string();
        for (secret, masked) in &self.replacements {
            if scrubbed.contains(secret.as_str()) {
                scrubbed = scrubbed.replace(secret.as_str(), masked);
            }
        }
        scrubbed
    }

    /// Scrubs every string contained in a JSON value, in place.
    ///
    /// Object keys are left untouched (they are field names, not secrets); only
    /// string values, including those nested in arrays and objects, are masked.
    pub fn scrub_value(&self, value: &mut Value) {
        match value {
            Value::String(text) => {
                if self
                    .replacements
                    .iter()
                    .any(|(secret, _)| text.contains(secret.as_str()))
                {
                    *text = self.scrub_text(text);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.scrub_value(item);
                }
            }
            Value::Object(map) => {
                for nested in map.values_mut() {
                    self.scrub_value(nested);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_unknown_secret_fully_preserving_length() {
        assert_eq!(mask("12345"), "*****");
        assert_eq!(mask(""), "");
        assert_eq!(mask("abcdefghij"), "**********");
    }

    #[test]
    fn keeps_known_prefix_and_last_four() {
        assert_eq!(mask("sk_live_123456789"), "sk_live_*****6789");
        assert_eq!(mask("sk-ant-abcdefghhijk"), "sk-ant-********hijk");
    }

    #[test]
    fn short_known_prefix_keeps_only_the_prefix() {
        // Not long enough to also reveal a tail without exposing most of it.
        assert_eq!(mask("ghp_ab"), "ghp_**");
        assert_eq!(mask("github_pat_12"), "github_pat_**");
    }

    #[test]
    fn scrubber_replaces_secret_in_text() {
        let scrubber = Scrubber::new(["github_pat_ABCDEFGH1234".to_string()]);
        let result = scrubber.scrub_text("the token is github_pat_ABCDEFGH1234 ok");
        assert_eq!(result, "the token is github_pat_********1234 ok");
    }

    #[test]
    fn scrubber_ignores_empty_and_too_short_values() {
        // Empty and sub-threshold values are dropped, so nothing is replaced.
        let scrubber = Scrubber::new([String::new(), "ab".to_string()]);
        assert_eq!(scrubber.scrub_text("ab cd"), "ab cd");
    }

    #[test]
    fn scrubber_walks_nested_json_values_but_not_keys() {
        let scrubber = Scrubber::new(["sk_live_123456789".to_string()]);
        let mut value = json!({
            "text": "key=sk_live_123456789",
            "nested": ["sk_live_123456789", 42],
            "sk_live_123456789": "untouched-key"
        });
        scrubber.scrub_value(&mut value);

        assert_eq!(value["text"], "key=sk_live_*****6789");
        assert_eq!(value["nested"][0], "sk_live_*****6789");
        assert_eq!(value["nested"][1], 42);
        // The key itself is a field name, not a value, so it is left as-is.
        assert_eq!(value["sk_live_123456789"], "untouched-key");
    }

    #[test]
    fn longer_secrets_are_scrubbed_before_shorter_substrings() {
        let scrubber = Scrubber::new(["abcdef".to_string(), "abcdef1234567890".to_string()]);
        let result = scrubber.scrub_text("abcdef1234567890");
        // The full-length secret wins, so we get its mask, not a partial one.
        assert_eq!(result, mask("abcdef1234567890"));
    }
}
