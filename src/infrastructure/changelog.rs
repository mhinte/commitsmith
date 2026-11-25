//! Writing the changelog file. The text is decided in the domain; this module
//! only puts it where it belongs and replaces the file in one step, so a crash
//! cannot leave half a changelog behind.

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::release::{SemVer, prepend};

pub const FILE: &str = "CHANGELOG.md";

/// Insert `section` for `version` at the top of `<root>/CHANGELOG.md`.
///
/// Returns the path that was written; the caller shows it in the status line.
pub fn write_section(root: &Path, version: &SemVer, section: &str) -> Result<PathBuf, String> {
    let path = root.join(FILE);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let text = prepend(&existing, version, section);
    if text == existing {
        // Already written: nothing to do, and nothing to report as an error.
        return Ok(path);
    }
    let temporary = root.join(format!(".{FILE}.tmp"));
    fs::write(&temporary, text).map_err(|error| format!("writing {FILE}: {error}"))?;
    fs::rename(&temporary, &path).map_err(|error| format!("replacing {FILE}: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-changelog-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_first_section_creates_the_file() {
        let dir = temp_dir("first");
        let path = write_section(&dir, &SemVer::new(0, 1, 0), "## 0.1.0\n\n- first").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Changelog"), "{text}");
        assert!(text.contains("## 0.1.0"), "{text}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_second_section_goes_on_top_and_the_first_stays() {
        let dir = temp_dir("second");
        write_section(&dir, &SemVer::new(0, 1, 0), "## 0.1.0\n\n- first").unwrap();
        write_section(&dir, &SemVer::new(0, 2, 0), "## 0.2.0\n\n- second").unwrap();
        let text = fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(text.find("## 0.2.0") < text.find("## 0.1.0"), "{text}");
        assert!(text.contains("- first"), "{text}");
        assert_eq!(text.matches("## 0.2.0").count(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn writing_the_same_version_twice_changes_nothing() {
        let dir = temp_dir("again");
        write_section(&dir, &SemVer::new(0, 1, 0), "## 0.1.0\n\n- first").unwrap();
        let before = fs::read_to_string(dir.join(FILE)).unwrap();
        write_section(&dir, &SemVer::new(0, 1, 0), "## 0.1.0\n\n- first").unwrap();
        let after = fs::read_to_string(dir.join(FILE)).unwrap();
        assert_eq!(before, after);
        let _ = fs::remove_dir_all(dir);
    }
}
