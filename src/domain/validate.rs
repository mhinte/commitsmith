//! Lint rules for Conventional Commits.
//!
//! `validate` checks an in-memory commit, `lint` checks a raw message and adds
//! the structural checks that only apply to text coming from git.

use indexmap::IndexSet;

use crate::domain::commit::{self, Commit, MAX_SUBJECT, RECOMMENDED_SUBJECT, TYPES};

/// Which types and limits apply. Built from the config so project files can
/// add types without touching this module.
#[derive(Debug, Clone)]
pub struct Rules {
    pub types: IndexSet<String>,
    pub max_subject: usize,
    pub recommended_subject: usize,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            types: TYPES.iter().map(|kind| (*kind).to_string()).collect(),
            max_subject: MAX_SUBJECT,
            recommended_subject: RECOMMENDED_SUBJECT,
        }
    }
}

impl Rules {
    /// Build from explicit values; the binary maps its config onto this so the
    /// domain never has to know what a config file is.
    pub fn new(
        types: impl IntoIterator<Item = String>,
        max_subject: usize,
        recommended_subject: usize,
    ) -> Self {
        Self {
            types: types.into_iter().collect(),
            max_subject,
            recommended_subject,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// Which part of the message the issue belongs to, e.g. `subject`.
    pub field: &'static str,
    pub message: String,
}

impl Issue {
    pub fn error(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            field,
            message: message.into(),
        }
    }

    pub fn warning(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            field,
            message: message.into(),
        }
    }

    pub fn info(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            field,
            message: message.into(),
        }
    }
}

/// Rules that need the parsed message as a whole.
#[cfg(test)]
pub fn lint(input: &str) -> Vec<Issue> {
    lint_with(input, &Rules::default())
}

pub fn lint_with(input: &str, rules: &Rules) -> Vec<Issue> {
    let parsed = commit::parse(input).with_upstream_header(input);
    let mut issues: Vec<Issue> = parsed
        .problems
        .into_iter()
        .map(|problem| Issue::error("header", problem))
        .collect();
    if parsed.header_ok {
        issues.extend(validate_with(&parsed.commit, rules));
    }
    issues
}

/// Rules that only look at the content of a single commit.
#[cfg(test)]
pub fn validate(commit: &Commit) -> Vec<Issue> {
    validate_with(commit, &Rules::default())
}

pub fn validate_with(commit: &Commit, rules: &Rules) -> Vec<Issue> {
    let mut issues = Vec::new();

    let kind = commit.kind.trim();
    if kind.is_empty() {
        issues.push(Issue::error("type", "pick a type, e.g. feat or fix"));
    } else if !rules.types.is_empty() && !rules.types.contains(kind) {
        issues.push(Issue::error(
            "type",
            format!(
                "`{kind}` is not a conventional type ({})",
                rules.types.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        ));
    }

    let scope = commit.scope.trim();
    if scope.contains(char::is_whitespace) {
        issues.push(Issue::warning("scope", "one word, no spaces (e.g. `api`)"));
    }
    if scope.chars().any(char::is_uppercase) {
        issues.push(Issue::warning(
            "scope",
            "lower case (e.g. `api`, not `API`)",
        ));
    }
    if commit.scope.contains('(') || commit.scope.contains(')') {
        issues.push(Issue::error(
            "scope",
            "no parentheses here, they belong to the header",
        ));
    }

    let subject = commit.subject.trim();
    let subject_len = subject.chars().count();
    if subject.is_empty() {
        issues.push(Issue::error(
            "subject",
            "empty: say what changes, in one line",
        ));
    } else {
        if subject_len > MAX_SUBJECT {
            issues.push(Issue::error(
                "subject",
                format!("{subject_len} characters, {MAX_SUBJECT} is the limit"),
            ));
        } else if subject_len > RECOMMENDED_SUBJECT {
            issues.push(Issue::warning(
                "subject",
                format!("{subject_len} characters, {RECOMMENDED_SUBJECT} reads better"),
            ));
        }
        if subject.ends_with('.') {
            issues.push(Issue::warning("subject", "no period at the end"));
        }
        if subject.chars().next().is_some_and(char::is_uppercase) {
            issues.push(Issue::warning(
                "subject",
                "lower case: the type already sets the tone",
            ));
        }
        if let Some(verb) = past_tense_verb(subject) {
            issues.push(Issue::warning(
                "subject",
                format!("imperative: `{}`, not `{verb}`", imperative(verb)),
            ));
        }
        if subject.contains("  ") {
            issues.push(Issue::warning("subject", "one space between words"));
        }
    }

    if commit.is_breaking() && commit.breaking.chars().count() < 10 {
        issues.push(Issue::warning(
            "breaking",
            "say what breaks and what to use instead",
        ));
    }

    if !has_errors(&issues) && commit.body.trim().is_empty() && commit.footer.trim().is_empty() {
        issues.push(Issue::info(
            "body",
            "optional: one line on the why helps reviewers",
        ));
    }

    for (field, text) in [("body", &commit.body), ("footer", &commit.footer)] {
        for (idx, line) in text.lines().enumerate() {
            let len = line.chars().count();
            if len > 100 {
                issues.push(Issue::warning(
                    field,
                    format!("line {} is {len} characters, 100 is the limit", idx + 1),
                ));
            }
        }
    }

    issues
}

const PAST_TENSE: [(&str, &str); 12] = [
    ("added", "add"),
    ("adds", "add"),
    ("fixed", "fix"),
    ("fixes", "fix"),
    ("changed", "change"),
    ("changes", "change"),
    ("updated", "update"),
    ("updates", "update"),
    ("removed", "remove"),
    ("removes", "remove"),
    ("implemented", "implement"),
    ("implements", "implement"),
];

fn past_tense_verb(subject: &str) -> Option<&'static str> {
    let first = subject.split_whitespace().next()?.to_ascii_lowercase();
    PAST_TENSE
        .iter()
        .find(|(verb, _)| *verb == first)
        .map(|(verb, _)| *verb)
}

fn imperative(verb: &'static str) -> &'static str {
    PAST_TENSE
        .iter()
        .find(|(candidate, _)| *candidate == verb)
        .map(|(_, imperative)| *imperative)
        .unwrap_or(verb)
}

/// Number of errors and warnings, for the status line.
pub fn count(issues: &[Issue]) -> (usize, usize) {
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == Severity::Error)
        .count();
    let warnings = issues
        .iter()
        .filter(|issue| issue.severity == Severity::Warning)
        .count();
    (errors, warnings)
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|issue| issue.severity == Severity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(kind: &str, subject: &str) -> Commit {
        Commit {
            kind: kind.into(),
            subject: subject.into(),
            ..Default::default()
        }
    }

    fn fields(issues: &[Issue]) -> Vec<&'static str> {
        issues.iter().map(|issue| issue.field).collect()
    }

    #[test]
    fn accepts_a_clean_commit() {
        let issues = validate(&commit("feat", "add commit composer"));
        assert!(!has_errors(&issues), "{issues:?}");
        assert_eq!(count(&issues).1, 0, "{issues:?}");
    }

    #[test]
    fn rejects_unknown_type() {
        let issues = validate(&commit("feature", "add composer"));
        assert!(issues.iter().any(|issue| issue.severity == Severity::Error
            && issue.message.contains("not a conventional type")));
    }

    #[test]
    fn rejects_empty_subject() {
        let issues = validate(&commit("feat", "  "));
        assert!(has_errors(&issues));
    }

    #[test]
    fn hints_once_about_a_missing_body() {
        let issues = validate_with(&commit("feat", "add commit composer"), &Rules::default());
        let hints: Vec<&Issue> = issues
            .iter()
            .filter(|issue| issue.severity == Severity::Info)
            .collect();
        assert_eq!(hints.len(), 1, "{issues:?}");
        assert_eq!(hints[0].field, "body");
    }

    #[test]
    fn warns_above_the_recommended_length() {
        let subject = "a".repeat(RECOMMENDED_SUBJECT + 1);
        let issues = validate(&commit("feat", &subject));
        assert!(
            issues
                .iter()
                .any(|issue| issue.severity == Severity::Warning)
        );
        assert!(!has_errors(&issues));
    }

    #[test]
    fn errors_above_the_hard_limit() {
        let subject = "a".repeat(MAX_SUBJECT + 1);
        assert!(has_errors(&validate(&commit("feat", &subject))));
    }

    #[test]
    fn warns_about_past_tense() {
        let issues = validate(&commit("feat", "added the composer"));
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("imperative:")),
            "{issues:?}"
        );
    }

    #[test]
    fn warns_about_trailing_period_and_capital() {
        let issues = validate(&commit("feat", "Add the composer."));
        assert_eq!(count(&issues).1, 2, "{issues:?}");
    }

    #[test]
    fn bang_without_breaking_footer_is_accepted() {
        // Conventional Commits allows `!` on its own; the footer is optional.
        let issues = lint("feat!: drop v1\n");
        assert!(!has_errors(&issues), "{issues:?}");
    }

    #[test]
    fn upstream_parser_accepts_bang_and_footer() {
        let issues = lint("feat(api)!: drop v1\n\nBREAKING CHANGE: use v2 instead\n");
        assert!(!has_errors(&issues), "{issues:?}");
        assert_eq!(count(&issues).1, 0, "{issues:?}");
    }

    #[test]
    fn accepts_a_full_message() {
        let issues = lint(
            "fix(api): stop dropping the trailing slash\n\n\
             The router stripped a trailing slash on redirects.\n\n\
             Refs: #42\n",
        );
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn unparseable_header_reports_one_cause_only() {
        let issues = lint("Fixed stuff.\n");
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].severity, Severity::Error);
    }

    #[test]
    fn empty_message_is_an_error() {
        assert!(has_errors(&lint("   \n\n")));
    }

    #[test]
    fn reports_unknown_type_for_raw_text() {
        let issues = lint("feature: add stuff\n");
        assert!(fields(&issues).contains(&"type"));
    }
}
