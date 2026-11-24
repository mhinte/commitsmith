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

fn forbidden_hits(dir: &str, forbidden: &[&str]) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
    let mut hits = Vec::new();
    for file in sources(&root) {
        let text = fs::read_to_string(&file).unwrap_or_default();
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
fn the_domain_layer_is_actually_there() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domain");
    let files = sources(&root);
    assert!(
        files.iter().any(|f| f.ends_with("ticket.rs")),
        "expected src/domain/ticket.rs, found {files:?}"
    );
}
