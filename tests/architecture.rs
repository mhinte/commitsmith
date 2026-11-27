//! Layering rules as tests.
//!
//! A comment saying "the domain has no dependencies" rots; this test does not.
//! It reads the sources and fails when a lower layer reaches upwards.

use std::fs;
use std::path::{Path, PathBuf};

fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(sources(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    found
}

/// Drop `//` and `/* */` comments: the rule is about what the code uses, and the
/// doc comments name the very crates the layer must not import.
fn code_only(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if depth == 0 && c == '/' && text[index..].starts_with("//") {
            // Skip to the end of the line.
            for (_, n) in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if c == '/' && text[index..].starts_with("/*") {
            depth += 1;
            chars.next();
            continue;
        }
        if depth > 0 {
            if c == '*' && text[index..].starts_with("*/") {
                depth -= 1;
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn forbidden_hits(dir: &str, forbidden: &[&str]) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
    let mut hits = Vec::new();
    for file in sources(&root) {
        let text = code_only(&fs::read_to_string(&file).unwrap_or_default());
        for needle in forbidden {
            if text.contains(needle) {
                hits.push(format!("{} uses {needle}", file.display()));
            }
        }
    }
    hits
}

#[test]
fn the_domain_layer_stays_free_of_ui_git_and_io() {
    // `std::process` and `std::fs` are the two ways a value object sneaks into
    // the outside world, so they are on the list as well.
    let hits = forbidden_hits(
        "src/domain",
        &[
            "ratatui",
            "crossterm",
            "gix",
            "std::process",
            "std::fs",
            "toml::",
        ],
    );
    assert!(hits.is_empty(), "{hits:#?}");
}

#[test]
fn the_application_layer_does_not_draw() {
    let hits = forbidden_hits(
        "src/application",
        &[
            "ratatui::Frame",
            "ratatui::widgets",
            "ratatui::layout",
            "ratatui::text",
            "DefaultTerminal",
        ],
    );
    assert!(hits.is_empty(), "{hits:#?}");
}

#[test]
fn the_ui_layer_does_not_read_git_or_spawn_processes() {
    let hits = forbidden_hits("src/ui", &["gix::", "std::process", "Command::new"]);
    assert!(hits.is_empty(), "{hits:#?}");
}

#[test]
fn the_domain_layer_is_actually_there() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domain");
    let files = sources(&root);
    assert!(
        files.iter().any(|f| f.ends_with("ticket.rs")),
        "expected src/domain/ticket.rs, found {files:?}"
    );
}

#[test]
fn the_repository_is_discovered_once_not_per_lookup() {
    // A `gix::discover` per highlighted commit meant a directory walk and a
    // config read for every keystroke. Discovery belongs to the adapter
    // (`infrastructure/gix_repo.rs`), nowhere else.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    for path in sources(&root) {
        if path.file_name().is_some_and(|name| name == "gix_repo.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        for (number, line) in text.lines().enumerate() {
            if line.trim().starts_with("//") {
                continue;
            }
            if line.contains("gix::discover") {
                offenders.push(format!("{}:{}", path.display(), number + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "discovery outside the adapter: {offenders:?}"
    );
}
