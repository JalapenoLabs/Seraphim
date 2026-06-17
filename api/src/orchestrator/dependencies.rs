//! Parsing and matching of a ticket's stated dependencies on other tickets.
//!
//! A ticket that builds on another ("A2 depends on A1") can name that dependency
//! with a `Depends on:` line in its body. When the dependency's pull request is
//! still open, its work is not yet on the default branch, so a branch cut from the
//! default branch can't build until the dependency is merged in (issue #256).
//!
//! This module turns the free-text marker into reference tokens and matches each
//! against an in-flight task, so the orchestrator can surface the dependency's PR
//! branch in the agent's brief. It is pure (no I/O), so the parsing and matching
//! rules are unit-testable in isolation; the orchestrator supplies the candidate
//! tasks and resolves the matched ones to PR branches.

use std::collections::HashSet;

/// Words shorter than this (after normalization) are dropped when matching, so a
/// stray single letter can't pull in an unrelated task. A roadmap key like `a1`
/// is two characters, so it survives.
const MIN_SIGNIFICANT_WORD_LEN: usize = 2;

/// Common words carrying no identifying signal, dropped from both sides before
/// the word-set match so they neither cause nor block a match.
const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "into", "ticket", "issue", "pr",
];

/// Extracts the dependency reference tokens from a ticket body.
///
/// Recognizes a `Depends on:` marker (also `Depends:`, `Dependency:`,
/// `Dependencies:`, with or without a trailing colon, case-insensitive, and
/// tolerant of leading list bullets and Markdown emphasis), then splits the rest
/// of that line into individual references on commas, semicolons, and the word
/// "and". Returns the trimmed tokens in order, deduped, empty when there is no
/// marker.
///
/// For example, `Depends on: A1 (package scaffold), A2 (#5)` yields
/// `["A1 (package scaffold)", "A2 (#5)"]`.
pub fn parse_dependency_refs(body: &str) -> Vec<String> {
    let mut refs: Vec<String> = Vec::new();
    for line in body.lines() {
        let Some(rest) = dependency_marker_rest(line) else {
            continue;
        };
        for token in split_references(rest) {
            let token = token.trim().to_string();
            // Skip empties and explicit "no dependency" placeholders.
            let lower = token.to_ascii_lowercase();
            if token.is_empty() || lower == "none" || lower == "n/a" || lower == "na" {
                continue;
            }
            if !refs.contains(&token) {
                refs.push(token);
            }
        }
    }
    refs
}

/// Returns the text following a `Depends on:`-style marker on `line`, or `None`
/// if the line is not such a marker. Strips leading list bullets / quote markers
/// and Markdown emphasis so `- **Depends on:** A1` is recognized.
fn dependency_marker_rest(line: &str) -> Option<&str> {
    // Longest markers first so "depends on" wins over "depends".
    const MARKERS: &[&str] = &["depends on", "dependencies", "dependency", "depends"];

    // Drop leading bullets, quote markers, emphasis, and whitespace. We trim from
    // the original slice (not a lowercased copy) so the returned text keeps its
    // original casing for display and number extraction.
    let trimmed = line.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '-' | '*' | '>' | '_' | '`' | '#' | '+')
    });
    let lower = trimmed.to_ascii_lowercase();
    for marker in MARKERS {
        if let Some(after) = lower.strip_prefix(marker) {
            // The marker may be followed by a colon and/or emphasis/whitespace; the
            // remainder must be separated from the marker (a colon or whitespace),
            // so "dependsonfoo" is not mistaken for a marker.
            let after_trimmed = after.trim_start_matches(|c: char| {
                c.is_whitespace() || matches!(c, ':' | '*' | '_' | '`')
            });
            if after_trimmed.len() == after.len() {
                continue; // No separator after the marker; not a real marker.
            }
            // Slice the original `trimmed` by the consumed byte length (ASCII
            // markers, so lengths line up with the lowercased copy).
            let consumed = trimmed.len() - after_trimmed.len();
            return Some(trimmed[consumed..].trim());
        }
    }
    None
}

/// Splits a dependency list into individual reference tokens on commas,
/// semicolons, and the standalone word "and".
fn split_references(list: &str) -> Vec<String> {
    list.split([',', ';'])
        .flat_map(|part| part.split(" and "))
        .map(|part| part.trim().to_string())
        .collect()
}

/// Whether a parsed reference token names the given candidate task.
///
/// Matches in two ways, either sufficient: an explicit GitHub issue number in the
/// reference (`#5`) equal to the candidate's `external_id` (only for GitHub
/// tasks), or a word-set overlap where one side's significant words are a subset
/// of the other's (so `A1 (package scaffold)` matches a task titled
/// `A1: Package scaffold`, and a bare `A1` matches it too, without `A1` matching
/// `A10`).
pub fn reference_matches(reference: &str, title: &str, external_id: &str, is_github: bool) -> bool {
    if is_github && reference_issue_numbers(reference).contains(external_id) {
        return true;
    }

    let reference_words = significant_words(reference);
    let title_words = significant_words(title);
    if reference_words.is_empty() || title_words.is_empty() {
        return false;
    }
    reference_words.is_subset(&title_words) || title_words.is_subset(&reference_words)
}

/// The set of `#`-prefixed issue numbers named in a reference, as strings (to
/// compare directly against a task's `external_id`).
fn reference_issue_numbers(reference: &str) -> HashSet<String> {
    let mut numbers = HashSet::new();
    for (index, character) in reference.char_indices() {
        if character != '#' {
            continue;
        }
        let digits: String = reference[index + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !digits.is_empty() {
            numbers.insert(digits);
        }
    }
    numbers
}

/// Normalizes text into its set of significant words: lowercased, with every
/// non-alphanumeric character treated as a separator, dropping stop words and
/// very short tokens so only identifying words remain.
fn significant_words(text: &str) -> HashSet<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= MIN_SIGNIFICANT_WORD_LEN && !STOP_WORDS.contains(word))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_inline_depends_on_line_with_multiple_refs() {
        let body = "Some description.\n\nDepends on: A1 (package scaffold), A2 and A3\n\nMore.";
        assert_eq!(
            parse_dependency_refs(body),
            vec![
                "A1 (package scaffold)".to_string(),
                "A2".to_string(),
                "A3".to_string()
            ]
        );
    }

    #[test]
    fn recognizes_bullets_emphasis_and_marker_variants() {
        assert_eq!(
            parse_dependency_refs("- **Depends on:** A1"),
            vec!["A1".to_string()]
        );
        assert_eq!(
            parse_dependency_refs("> Dependencies: foo, bar"),
            vec!["foo".to_string(), "bar".to_string()]
        );
        assert_eq!(
            parse_dependency_refs("Depends A1"), // no colon, separated by space
            vec!["A1".to_string()]
        );
    }

    #[test]
    fn ignores_non_markers_and_placeholders() {
        assert!(parse_dependency_refs("This depends on nothing in prose.").is_empty());
        assert!(parse_dependency_refs("Independent of other work").is_empty());
        assert!(parse_dependency_refs("Depends on: none").is_empty());
        assert!(parse_dependency_refs("No marker here").is_empty());
    }

    #[test]
    fn matches_by_issue_number_only_for_github() {
        assert!(reference_matches("A2 (#5)", "Unrelated title", "5", true));
        // Same token against a non-GitHub task does not number-match.
        assert!(!reference_matches("A2 (#5)", "Unrelated title", "5", false));
        // A different number does not match.
        assert!(!reference_matches("see #6", "Unrelated", "5", true));
    }

    #[test]
    fn matches_by_title_word_set() {
        // Full key + parenthetical matches the task title both ways.
        assert!(reference_matches(
            "A1 (package scaffold)",
            "A1: Package scaffold",
            "4",
            true
        ));
        // A bare roadmap key matches a title that begins with it.
        assert!(reference_matches("A1", "A1: Package scaffold", "4", true));
    }

    #[test]
    fn does_not_match_unrelated_or_lookalike_keys() {
        // `A1` must not match `A10`.
        assert!(!reference_matches("A1", "A10: Something else", "9", true));
        // No shared significant words.
        assert!(!reference_matches(
            "A1 (package scaffold)",
            "B2: Wire up the SDK",
            "7",
            true
        ));
    }
}
