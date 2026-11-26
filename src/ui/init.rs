//! `commitsmith init`: the questions, and the plain-text loop that asks them.
//!
//! The dialog does not need the TUI: it runs before any repository is open, in a
//! terminal that may not even be a terminal (a pipe, `--defaults`, a test). It
//! therefore reads lines from stdin and writes lines to stdout.

use std::io::{self, BufRead, Write};

use commitsmith::domain::setup::SetupAnswers;

/// What the wizard should do.
pub struct Options {
    /// Take the defaults without asking.
    pub defaults: bool,
}

/// Ask for the settings and return them, or `None` when the user aborted.
pub fn ask(
    options: &Options,
    profiles: &[String],
    current: Option<&SetupAnswers>,
) -> Option<SetupAnswers> {
    let mut answers = current
        .cloned()
        .unwrap_or_else(|| SetupAnswers::defaults(profiles));
    if options.defaults {
        return Some(answers);
    }

    println!("commitsmith setup");
    println!("Enter keeps the value in brackets.\n");

    let author = prompt("Author shortcut for branches", answers.author.as_str());
    match answers.clone().with_author(&author) {
        Ok(next) => answers = next,
        Err(error) => {
            eprintln!("  {error}");
            return None;
        }
    }

    let prefix = prompt("Ticket prefix", &answers.ticket_prefix);
    match answers.clone().with_ticket_prefix(&prefix) {
        Ok(next) => answers = next,
        Err(error) => {
            eprintln!("  {error}");
            return None;
        }
    }

    let reference = prompt(
        "Reference keyword (Refs/Closes)",
        answers.reference.as_str(),
    );
    match answers.clone().with_reference(&reference) {
        Ok(next) => answers = next,
        Err(error) => {
            eprintln!("  {error}");
            return None;
        }
    }

    let template = prompt("Branch template", answers.branch_template.as_str());
    match answers.clone().with_template(&template) {
        Ok(next) => answers = next,
        Err(error) => {
            eprintln!("  {error}");
            return None;
        }
    }

    let profile = prompt("Commit profile", &answers.profile);
    match answers.clone().with_profile(&profile, profiles) {
        Ok(next) => answers = next,
        Err(error) => {
            eprintln!("  {error}");
            return None;
        }
    }

    answers.emoji = confirm("Emojis in the type picker", answers.emoji);
    answers.co_author = confirm(
        "Add `Co-authored-by:` to the commit footer",
        answers.co_author,
    );
    answers.git_alias = confirm("Offer `git smith` as a git alias", answers.git_alias);
    Some(answers)
}

fn prompt(label: &str, default: &str) -> String {
    print!("{label} [{default}]: ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return default.to_string();
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn confirm(label: &str, default: bool) -> bool {
    let shown = if default { "Y/n" } else { "y/N" };
    print!("{label} [{shown}]: ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return default;
    }
    match line.trim().to_lowercase().as_str() {
        "y" | "yes" | "j" | "ja" => true,
        "n" | "no" | "nein" => false,
        // An empty answer or anything unrecognised keeps the default.
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Vec<String> {
        vec!["conventional".into(), "angular".into()]
    }

    #[test]
    fn defaults_skip_every_question() {
        let answers = ask(&Options { defaults: true }, &profiles(), None).unwrap();
        assert_eq!(answers.ticket_prefix, "TICKET");
        assert_eq!(answers.profile, "conventional");
    }

    #[test]
    fn an_existing_answer_is_the_starting_point() {
        let current = SetupAnswers::defaults(&profiles())
            .with_author("mhi")
            .unwrap()
            .with_ticket_prefix("ABC")
            .unwrap();
        let answers = ask(&Options { defaults: true }, &profiles(), Some(&current)).unwrap();
        assert_eq!(answers.author.as_str(), "mhi");
        assert_eq!(answers.ticket_prefix, "ABC");
    }
}
