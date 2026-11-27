//! Filtering the conventional log.
//!
//! The rules live here, not in the pane: a filter is a question about commits, and
//! the answer must be the same whether a test asks it or the list does.

use super::history::PastCommit;

/// The three filters of tab 3, combinable. `None` means "do not care".
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Filter {
    pub kind: Option<String>,
    pub scope: Option<String>,
    /// Matched against the whole message, because tickets live in the footer as
    /// often as in the subject.
    pub ticket: Option<String>,
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        self.kind.is_none() && self.scope.is_none() && self.ticket.is_none()
    }

    /// True when the commit satisfies every set part of the filter.
    pub fn matches(&self, commit: &PastCommit) -> bool {
        let kind_ok = match &self.kind {
            None => true,
            Some(wanted) => commit
                .kind
                .as_deref()
                .map(|kind| kind.eq_ignore_ascii_case(wanted))
                .unwrap_or(false),
        };
        let scope_ok = match &self.scope {
            None => true,
            Some(wanted) => commit
                .scope
                .as_deref()
                .map(|scope| scope.eq_ignore_ascii_case(wanted))
                .unwrap_or(false),
        };
        let ticket_ok = match &self.ticket {
            None => true,
            Some(wanted) => {
                let needle = wanted.to_lowercase();
                let haystack = format!("{} {}", commit.summary, commit.body).to_lowercase();
                haystack.contains(&needle)
            }
        };
        kind_ok && scope_ok && ticket_ok
    }

    /// One line for the pane title: what is currently being asked.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(kind) = &self.kind {
            parts.push(format!("type: {kind}"));
        }
        if let Some(scope) = &self.scope {
            parts.push(format!("scope: {scope}"));
        }
        if let Some(ticket) = &self.ticket {
            parts.push(format!("ticket: {ticket}"));
        }
        parts.join(" · ")
    }
}

/// Turn what was typed into the filter line into a [`Filter`].
///
/// `type:feat scope:ui` and the shorter `feat ui` mean the same; a word that is
/// not a known type is read as a ticket, because that is what people type when
/// they hunt for one.
pub fn parse_filter(input: &str, known_types: &[String]) -> Filter {
    let mut filter = Filter::default();
    for word in input.split_whitespace() {
        if let Some(value) = word
            .strip_prefix("type:")
            .or_else(|| word.strip_prefix("t:"))
        {
            filter.kind = Some(value.to_string());
        } else if let Some(value) = word
            .strip_prefix("scope:")
            .or_else(|| word.strip_prefix("s:"))
        {
            filter.scope = Some(value.to_string());
        } else if let Some(value) = word
            .strip_prefix("ticket:")
            .or_else(|| word.strip_prefix("i:"))
        {
            filter.ticket = Some(value.to_string());
        } else if known_types
            .iter()
            .any(|kind| kind.eq_ignore_ascii_case(word))
        {
            filter.kind = Some(word.to_string());
        } else if filter.ticket.is_none() {
            filter.ticket = Some(word.to_string());
        }
    }
    filter
}

/// The commits that pass, in the order they were given (newest first).
pub fn filter_commits<'a>(commits: &'a [PastCommit], filter: &Filter) -> Vec<&'a PastCommit> {
    commits
        .iter()
        .filter(|commit| filter.matches(commit))
        .collect()
}

/// Distinct values in the order they appear, for cycling through them.
pub fn distinct_kinds(commits: &[PastCommit]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for commit in commits {
        if let Some(kind) = &commit.kind {
            if !seen.iter().any(|value| value.eq_ignore_ascii_case(kind)) {
                seen.push(kind.clone());
            }
        }
    }
    seen
}

pub fn distinct_scopes(commits: &[PastCommit]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for commit in commits {
        if let Some(scope) = &commit.scope {
            if !seen.iter().any(|value| value.eq_ignore_ascii_case(scope)) {
                seen.push(scope.clone());
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(kind: Option<&str>, scope: Option<&str>, summary: &str, body: &str) -> PastCommit {
        PastCommit {
            short_id: "abc1234".to_string(),
            id: "abc1234".to_string(),
            author: "Marc".to_string(),
            date: "today".to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            kind: kind.map(std::string::ToString::to_string),
            scope: scope.map(std::string::ToString::to_string),
            files: Vec::new(),
            merge: false,
        }
    }

    #[test]
    fn no_filter_keeps_everything() {
        let commits = vec![
            commit(Some("feat"), None, "a", ""),
            commit(None, None, "b", ""),
        ];
        let filter = Filter::default();
        assert!(filter.is_empty());
        assert_eq!(filter_commits(&commits, &filter).len(), 2);
    }

    #[test]
    fn the_parts_combine_and_ignore_case() {
        let commits = vec![
            commit(Some("feat"), Some("UI"), "add a button", ""),
            commit(Some("feat"), Some("api"), "add a route", ""),
            commit(Some("fix"), Some("ui"), "correct a label", ""),
        ];
        let filter = Filter {
            kind: Some("FEAT".to_string()),
            scope: Some("ui".to_string()),
            ticket: None,
        };
        let hits = filter_commits(&commits, &filter);
        assert_eq!(hits.len(), 1, "only the feat in ui");
        assert_eq!(hits[0].summary, "add a button");
    }

    #[test]
    fn a_ticket_is_found_in_the_subject_or_the_body() {
        let commits = vec![
            commit(Some("feat"), None, "add TICKET-42 support", ""),
            commit(Some("fix"), None, "repair it", "Refs TICKET-42"),
            commit(Some("chore"), None, "no ticket", ""),
        ];
        let filter = Filter {
            ticket: Some("ticket-42".to_string()),
            ..Filter::default()
        };
        assert_eq!(filter_commits(&commits, &filter).len(), 2);
    }

    #[test]
    fn a_scope_filter_excludes_unscoped_commits() {
        let commits = vec![commit(Some("feat"), None, "no scope", "")];
        let filter = Filter {
            scope: Some("ui".to_string()),
            ..Filter::default()
        };
        assert!(filter_commits(&commits, &filter).is_empty());
    }

    #[test]
    fn the_description_names_what_is_set() {
        let filter = Filter {
            kind: Some("fix".to_string()),
            scope: Some("ui".to_string()),
            ticket: None,
        };
        assert_eq!(filter.describe(), "type: fix · scope: ui");
    }

    #[test]
    fn distinct_values_keep_their_order_and_come_once() {
        let commits = vec![
            commit(Some("feat"), Some("ui"), "a", ""),
            commit(Some("fix"), Some("ui"), "b", ""),
            commit(Some("Feat"), Some("api"), "c", ""),
        ];
        assert_eq!(distinct_kinds(&commits), vec!["feat", "fix"]);
        assert_eq!(distinct_scopes(&commits), vec!["ui", "api"]);
    }
}
