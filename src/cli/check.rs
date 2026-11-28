use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use super::load_config;

use crate::validate;
use crate::validate::Severity;

pub(crate) fn run_check(
    file: Option<PathBuf>,
    message: Option<String>,
    strict: bool,
    fix: bool,
) -> ExitCode {
    let file_path = file.clone();
    let mut text = match (file, message) {
        (Some(path), _) => match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("commitsmith: cannot read {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        },
        (None, Some(message)) => message,
        (None, None) => {
            let mut buffer = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut buffer) {
                eprintln!("commitsmith: cannot read stdin: {error}");
                return ExitCode::FAILURE;
            }
            buffer
        }
    };

    // `--fix` writes the repaired message back into the file the hook was given and
    // reports one line per correction. It repairs, it never decides: the lint below runs
    // on the result, so a message that is wrong for a reason the repair cannot know is
    // still refused.
    if fix {
        let Some(path) = file_path.as_deref() else {
            eprintln!("commitsmith: --fix needs --file");
            return ExitCode::FAILURE;
        };
        let repaired = commitsmith::domain::repair::repair(&text);
        for correction in &repaired.fixes {
            eprintln!("fixed: {}", correction.what);
        }
        if !repaired.fixes.is_empty() {
            if let Err(error) = std::fs::write(path, &repaired.text) {
                eprintln!("commitsmith: cannot write {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
            text = repaired.text;
        }
    }

    let (config, warnings) = load_config();
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    let rules = validate::rules_from_config(&config);
    let issues = validate::lint_with(&text, &rules);
    let (errors, warnings) = validate::count(&issues);

    for issue in &issues {
        let symbol = match issue.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        eprintln!("{symbol}: [{}] {}", issue.field, issue.message);
    }

    if issues.is_empty() {
        println!("commit message is fine");
    } else {
        println!("{errors} errors, {warnings} warnings");
    }

    if errors > 0 || (strict && warnings > 0) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
