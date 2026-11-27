//! Fixing what is unambiguous in a commit message, and nothing else.
//!
//! A colleague who writes `Feat(API):  add the thing.` does not want a lecture, they
//! want the commit. Three rules apply, and each has to be provable neutral:
//!
//! - a single trailing period on the subject goes (Conventional Commits asks for none);
//! - the type is written lower case, the scope too when it is not a ticket;
//! - spacing is normalised (`feat :x` → `feat: x`, double spaces collapse).
//!
//! Nothing that changes wording is touched: no shortening, no rewording, no touching
//! the body, and never a header that does not parse — a message with no `:` is a
//! question for the author, not something to guess at.

use crate::domain::commit;

/// One correction, in words a person reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// What was wrong, for the report: `"type: \`Feat\` -> \`feat\`"`.
    pub what: String,
}

/// The repaired message and every correction made. An empty `fixes` means the input
/// was left exactly as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repaired {
    pub text: String,
    pub fixes: Vec<Fix>,
}

/// Repair `input`. The body and every footer line stay byte for byte as they are: only
/// the first non-comment line is examined.
pub fn repair(input: &str) -> Repaired {
    let mut fixes = Vec::new();
    let mut lines: Vec<String> = input.lines().map(str::to_string).collect();
    let Some(index) = lines
        .iter()
        .position(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
    else {
        return Repaired {
            text: input.to_string(),
            fixes,
        };
    };

    let original = lines[index].clone();
    let mut header = original.trim_end().to_string();
    if header != original {
        fixes.push(Fix {
            what: "trailing whitespace removed".to_string(),
        });
    }

    // Without a colon this is not a header we understand; guessing would be worse
    // than saying so.
    let Some((prefix, subject)) = header
        .clone()
        .split_once(':')
        .map(|(a, b)| (a.to_string(), b.to_string()))
    else {
        return Repaired {
            text: input.to_string(),
            fixes: Vec::new(),
        };
    };

    let (fixed_prefix, prefix_fixes) = repair_prefix(&prefix);
    let (fixed_subject, subject_fixes) = repair_subject(&subject);
    fixes.extend(prefix_fixes);
    fixes.extend(subject_fixes);
    header = format!("{fixed_prefix}:{fixed_subject}");

    if fixes.is_empty() && header == original {
        return Repaired {
            text: input.to_string(),
            fixes,
        };
    }

    // A header that still does not parse is left alone: the repair must never turn one
    // broken message into another.
    if !commit::parse(&header).header_ok {
        return Repaired {
            text: input.to_string(),
            fixes: Vec::new(),
        };
    }

    lines[index] = header;
    Repaired {
        // `lines()` dropped a trailing newline; put it back so a hook writing the file
        // cannot silently change its last line.
        text: {
            let mut text = lines.join("\n");
            if input.ends_with('\n') {
                text.push('\n');
            }
            text
        },
        fixes,
    }
}

/// `Feat ( API )` → `feat(API)`: case of the type, spacing around the parentheses.
fn repair_prefix(prefix: &str) -> (String, Vec<Fix>) {
    let mut fixes = Vec::new();
    let trimmed = prefix.trim();
    if trimmed != prefix {
        fixes.push(Fix {
            what: "space before `:` removed".to_string(),
        });
    }
    let (type_part, scope_part) = match trimmed.split_once('(') {
        Some((kind, rest)) => (kind.trim(), Some(rest.trim_end_matches(')').trim())),
        None => (trimmed, None),
    };
    let mut fixed_type = type_part.to_string();
    if fixed_type.chars().any(char::is_uppercase) {
        fixed_type = fixed_type.to_lowercase();
        fixes.push(Fix {
            what: format!("type `{type_part}` -> `{fixed_type}`"),
        });
    }
    match scope_part {
        Some(scope) => {
            // A ticket keeps its spelling; a module name goes lower case, the same rule
            // the linter applies.
            let is_ticket =
                crate::domain::ticket::in_text(scope).is_some_and(|found| found == scope);
            let mut fixed_scope = scope.to_string();
            if !is_ticket && fixed_scope.chars().any(char::is_uppercase) {
                fixed_scope = fixed_scope.to_lowercase();
                fixes.push(Fix {
                    what: format!("scope `{scope}` -> `{fixed_scope}`"),
                });
            }
            (format!("{fixed_type}({fixed_scope})"), fixes)
        }
        None => (fixed_type, fixes),
    }
}

/// `  add the thing.` → ` add the thing`: one leading space, no trailing period.
fn repair_subject(subject: &str) -> (String, Vec<Fix>) {
    let mut fixes = Vec::new();
    let mut text = subject.trim_start().to_string();
    if subject != format!(" {text}") && subject.trim() == text {
        fixes.push(Fix {
            what: "spacing after `:` normalised".to_string(),
        });
    }
    while text.contains("  ") {
        text = text.replace("  ", " ");
        fixes.push(Fix {
            what: "double space collapsed".to_string(),
        });
    }
    // One period at the end, and only if something else remains: `.` alone is not a
    // subject the repair should empty out.
    if text.ends_with('.') && !text.ends_with("..") && !text.trim_end_matches('.').trim().is_empty()
    {
        text.pop();
        fixes.push(Fix {
            what: "trailing period removed".to_string(),
        });
    }
    (format!(" {text}"), fixes)
}

#[cfg(test)]
mod tests {
    use super::repair;

    fn fixed(input: &str) -> String {
        repair(input).text
    }

    #[test]
    fn a_trailing_period_goes() {
        let repaired = repair("feat: add a thing.\n");
        assert_eq!(repaired.text, "feat: add a thing\n");
        assert_eq!(repaired.fixes.len(), 1);
        assert!(repaired.fixes[0].what.contains("period"));
    }

    #[test]
    fn the_type_and_a_module_scope_go_lower_case() {
        assert_eq!(fixed("Feat: add a thing\n"), "feat: add a thing\n");
        assert_eq!(fixed("fix(API): add a thing\n"), "fix(api): add a thing\n");
    }

    #[test]
    fn a_ticket_in_the_scope_keeps_its_case() {
        // Written by hand as a scope: the ticket system spells it upper case.
        assert_eq!(
            fixed("fix(OPS-8): fix a typo\n"),
            "fix(OPS-8): fix a typo\n"
        );
    }

    #[test]
    fn spacing_is_normalised() {
        assert_eq!(fixed("feat :add a thing\n"), "feat: add a thing\n");
        assert_eq!(fixed("feat:  add  a thing\n"), "feat: add a thing\n");
        assert_eq!(fixed("feat:add a thing\n"), "feat: add a thing\n");
    }

    #[test]
    fn several_faults_in_one_line_are_all_fixed() {
        let repaired = repair("Feat(API) :  Add the thing.\n");
        assert_eq!(repaired.text, "feat(api): Add the thing\n");
        assert!(repaired.fixes.len() >= 3, "{:?}", repaired.fixes);
    }

    #[test]
    fn a_clean_message_is_untouched_and_reports_nothing() {
        let clean = "feat(api): add rate limit\n\nA body line.\n\nRefs TICKET-1\n";
        let repaired = repair(clean);
        assert_eq!(repaired.text, clean);
        assert!(repaired.fixes.is_empty(), "{:?}", repaired.fixes);
    }

    #[test]
    fn the_body_and_footers_are_never_touched() {
        // A period in the body or a capital in a footer is not this tool's business.
        let input = "Feat: add a thing\n\nThe body has a period.\n\nRefs TICKET-9\n";
        let repaired = repair(input);
        assert_eq!(
            repaired.text,
            "feat: add a thing\n\nThe body has a period.\n\nRefs TICKET-9\n"
        );
    }

    #[test]
    fn a_message_without_a_colon_is_left_for_the_author() {
        let input = "Add a thing\n";
        let repaired = repair(input);
        assert_eq!(repaired.text, input, "no guessing at a missing type");
        assert!(repaired.fixes.is_empty());
    }

    #[test]
    fn nothing_is_emptied_or_shortened() {
        // A dot-only subject must not become an empty one.
        assert_eq!(fixed("feat: .\n"), "feat: .\n");
        // Words are never changed, only case and spacing.
        assert_eq!(
            fixed("Fix: Corrected the Thing\n"),
            "fix: Corrected the Thing\n"
        );
    }

    #[test]
    fn a_comment_block_keeps_its_place() {
        let input = "# comment\nFeat:  add a thing.\n# more\n";
        assert_eq!(repair(input).text, "# comment\nfeat: add a thing\n# more\n");
    }

    #[test]
    fn a_missing_final_newline_stays_missing() {
        assert_eq!(fixed("feat: add a thing."), "feat: add a thing");
    }
}
