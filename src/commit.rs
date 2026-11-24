//! The Conventional Commit model: composing, rendering and parsing messages.
//!
//! Spec: <https://www.conventionalcommits.org/en/v1.0.0/>

/// Commit types the composer offers. Linting accepts exactly these.
pub const TYPES: [&str; 11] = [
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

/// Hard limit for the subject line, recommended by the spec ecosystem.
pub const MAX_SUBJECT: usize = 72;
/// Beyond this length git log and GitHub start to truncate.
pub const RECOMMENDED_SUBJECT: usize = 50;
/// Wrap width for body and footer text.
pub const BODY_WRAP: usize = 72;

/// A commit message split into its parts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Commit {
    pub kind: String,
    pub scope: String,
    pub subject: String,
    pub body: String,
    pub breaking: String,
    pub footer: String,
}

impl Commit {
    /// The `type(scope)!: subject` line.
    pub fn header(&self) -> String {
        let kind = self.kind.trim();
        let scope = self.scope.trim();
        let bang = if self.is_breaking() { "!" } else { "" };
        if scope.is_empty() {
            format!("{kind}{bang}: {}", self.subject.trim())
        } else {
            format!("{kind}({scope}){bang}: {}", self.subject.trim())
        }
    }

    /// True when a `BREAKING CHANGE` footer will be written.
    pub fn is_breaking(&self) -> bool {
        !self.breaking.trim().is_empty()
    }

    /// The full message, ready for `git commit -m`.
    pub fn render(&self) -> String {
        let mut parts = vec![self.header()];

        let body = self.body.trim();
        if !body.is_empty() {
            parts.push(wrap(body, BODY_WRAP).join("\n"));
        }
        if self.is_breaking() {
            let text = wrap(self.breaking.trim(), BODY_WRAP - "BREAKING CHANGE: ".len());
            parts.push(format!("BREAKING CHANGE: {}", text.join("\n")));
        }
        let footer = self.footer.trim();
        if !footer.is_empty() {
            parts.push(wrap(footer, BODY_WRAP).join("\n"));
        }
        parts.join("\n\n")
    }
}

/// Result of parsing an existing message. Structural problems are collected
/// instead of aborting, so a linter can report all of them at once.
#[derive(Debug, Default)]
pub struct Parsed {
    pub commit: Commit,
    pub problems: Vec<String>,
    /// False when the header could not be split at all, so semantic rules
    /// would only produce follow-up noise.
    pub header_ok: bool,
}

/// Parse a commit message as produced by `git commit` or an editor.
///
/// Comment lines (`#`) and surrounding blank lines are ignored, like git does
/// with `core.commentChar`.
pub fn parse(input: &str) -> Parsed {
    let lines: Vec<&str> = input
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect();

    let mut parsed = Parsed::default();
    let header_idx = match lines.iter().position(|line| !line.trim().is_empty()) {
        Some(idx) => idx,
        None => {
            parsed.problems.push("message is empty".to_string());
            return parsed;
        }
    };

    let header = lines[header_idx];
    if header.trim_end() != header {
        parsed
            .problems
            .push("header has trailing whitespace".to_string());
    }

    let (prefix, subject) = match header.split_once(':') {
        Some((prefix, subject)) => (prefix.trim(), subject),
        None => {
            parsed
                .problems
                .push("header must look like `type(scope): subject`".to_string());
            return parsed;
        }
    };

    if !subject.starts_with(' ') {
        parsed
            .problems
            .push("subject must be separated from the type by `: `".to_string());
    }
    let subject = subject.trim_start();

    parsed.header_ok = true;
    let (kind, scope, bang) = split_prefix(prefix);
    parsed.commit.kind = kind.to_string();
    parsed.commit.scope = scope.to_string();
    parsed.commit.subject = subject.trim_end().to_string();

    let rest: Vec<&str> = lines[header_idx + 1..].to_vec();
    let first_content = rest.iter().position(|line| !line.trim().is_empty());
    if let Some(idx) = first_content {
        if idx == 0 && rest.iter().any(|line| !line.trim().is_empty()) {
            parsed
                .problems
                .push("leave one blank line between the subject and the body".to_string());
        }
    }

    let mut body: Vec<&str> = Vec::new();
    let mut footer: Vec<&str> = Vec::new();
    let mut in_footer = false;
    for line in rest.iter().skip_while(|line| line.trim().is_empty()) {
        if is_footer(line) {
            in_footer = true;
        }
        if in_footer {
            footer.push(line);
        } else {
            body.push(line);
        }
    }

    let body_text = body.join("\n");
    let mut footer_text = footer.join("\n");

    // Pull the breaking change out of the footer so the header gets its `!`.
    if let Some(idx) = footer.iter().position(|line| is_breaking_footer(line)) {
        let mut breaking_lines = vec![strip_breaking_footer(footer[idx]).trim().to_string()];
        let mut end = idx + 1;
        while end < footer.len() {
            let line = footer[end];
            if line.trim().is_empty() || is_footer(line) {
                break;
            }
            breaking_lines.push(line.trim().to_string());
            end += 1;
        }
        parsed.commit.breaking = breaking_lines.join(" ").trim().to_string();

        let mut remaining: Vec<&str> = footer[..idx].to_vec();
        remaining.extend_from_slice(&footer[end..]);
        footer_text = remaining.join("\n");
    }

    parsed.commit.body = body_text.trim().to_string();
    parsed.commit.footer = footer_text.trim().to_string();

    if bang && !parsed.commit.is_breaking() {
        parsed
            .problems
            .push("header uses `!` but no BREAKING CHANGE footer explains the break".to_string());
    }

    parsed
}

fn split_prefix(prefix: &str) -> (&str, &str, bool) {
    let bang = prefix.ends_with('!');
    let prefix = prefix.trim_end_matches('!');
    match prefix.split_once('(') {
        Some((kind, rest)) => {
            let scope = rest.trim_end_matches(')');
            (kind.trim(), scope.trim(), bang)
        }
        None => (prefix.trim(), "", bang),
    }
}

fn is_breaking_footer(line: &str) -> bool {
    let upper = line.to_ascii_uppercase();
    upper.starts_with("BREAKING CHANGE:") || upper.starts_with("BREAKING-CHANGE:")
}

fn strip_breaking_footer(line: &str) -> &str {
    match line.split_once(':') {
        Some((_, rest)) => rest,
        None => "",
    }
}

/// A footer token is `Word:` or `Word-With-Dashes:` at the start of a line.
fn is_footer(line: &str) -> bool {
    if is_breaking_footer(line) {
        return true;
    }
    match line.split_once(": ") {
        Some((token, _)) => {
            !token.is_empty()
                && token.len() <= 24
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && token
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Word-wrap `text` to `width` characters, preserving existing line breaks.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            if word_len > width {
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                }
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(width) {
                    out.push(chunk.iter().collect());
                }
                continue;
            }
            if line.is_empty() {
                line.push_str(word);
            } else if line.chars().count() + 1 + word_len <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                out.push(std::mem::take(&mut line));
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_minimal_commit() {
        let commit = Commit {
            kind: "feat".into(),
            subject: "add commit composer".into(),
            ..Default::default()
        };
        assert_eq!(commit.render(), "feat: add commit composer");
    }

    #[test]
    fn renders_scope_body_and_footers() {
        let commit = Commit {
            kind: "fix".into(),
            scope: "api".into(),
            subject: "stop dropping the trailing slash".into(),
            body: "The router stripped a trailing slash on redirects.".into(),
            breaking: "clients must send absolute paths".into(),
            footer: "Refs: #42".into(),
        };
        assert_eq!(
            commit.render(),
            "fix(api)!: stop dropping the trailing slash\n\n\
             The router stripped a trailing slash on redirects.\n\n\
             BREAKING CHANGE: clients must send absolute paths\n\n\
             Refs: #42"
        );
    }

    #[test]
    fn parses_type_scope_and_subject() {
        let parsed = parse("feat(ui): add preview pane\n");
        assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
        assert_eq!(parsed.commit.kind, "feat");
        assert_eq!(parsed.commit.scope, "ui");
        assert_eq!(parsed.commit.subject, "add preview pane");
    }

    #[test]
    fn parses_breaking_change_footer() {
        let parsed = parse(
            "refactor(core)!: drop the legacy parser\n\n\
             BREAKING CHANGE: remove `parse_v1`\n\n\
             Refs: #7\n",
        );
        assert_eq!(parsed.commit.breaking, "remove `parse_v1`");
        assert_eq!(parsed.commit.footer, "Refs: #7");
    }

    #[test]
    fn round_trips_a_full_message() {
        let original = Commit {
            kind: "feat".into(),
            scope: "composer".into(),
            subject: "highlight invalid fields".into(),
            body: "Fields with errors are rendered in red until they are fixed.".into(),
            breaking: String::new(),
            footer: "Refs: #12".into(),
        };
        let parsed = parse(&original.render());
        assert_eq!(parsed.commit, original);
    }

    #[test]
    fn reports_missing_separator() {
        let parsed = parse("feat:add stuff");
        assert!(
            parsed
                .problems
                .iter()
                .any(|problem| problem.contains("`: `")),
            "{:?}",
            parsed.problems
        );
    }

    #[test]
    fn reports_missing_blank_line() {
        let parsed = parse("feat: add stuff\nbody without blank line\n");
        assert!(
            parsed
                .problems
                .iter()
                .any(|problem| problem.contains("blank line")),
            "{:?}",
            parsed.problems
        );
    }

    #[test]
    fn wraps_long_lines() {
        let text = "word ".repeat(40);
        let lines = wrap(text.trim(), 20);
        assert!(lines.iter().all(|line| line.chars().count() <= 20));
    }

    #[test]
    fn breaks_words_longer_than_the_limit() {
        let lines = wrap("a".repeat(30).as_str(), 10);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.chars().count() <= 10));
    }

    #[test]
    fn ignores_comment_lines() {
        let parsed = parse("# Please enter the commit message\nfeat: add x\n");
        assert_eq!(parsed.commit.kind, "feat");
    }
}
