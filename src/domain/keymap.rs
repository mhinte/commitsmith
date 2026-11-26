//! Which keys the footer and the help put first.
//!
//! Two profiles, one vocabulary: the same keys work everywhere, the profile only
//! decides what is advertised where. `default` is the terminal-first set
//! (`Ctrl+S` writes, `Ctrl+N` suggests); `ide` leads with the pair every editor
//! has (`Ctrl+Enter`, `Ctrl+Space`).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Keymap {
    #[default]
    Default,
    Ide,
}

impl Keymap {
    /// Read from config; anything unknown keeps the default.
    pub fn parse(text: &str) -> Self {
        match text.trim().to_lowercase().as_str() {
            "ide" | "editor" | "intellij" => Self::Ide,
            _ => Self::Default,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Ide => "ide",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_is_read_leniently_and_prints_canonically() {
        assert_eq!(Keymap::parse("IDE"), Keymap::Ide);
        assert_eq!(Keymap::parse("intellij"), Keymap::Ide);
        assert_eq!(Keymap::parse("vim"), Keymap::Default);
        assert_eq!(Keymap::parse("  ide  "), Keymap::Ide);
        assert_eq!(Keymap::Ide.as_str(), "ide");
        assert_eq!(Keymap::Default.as_str(), "default");
    }
}
