//! Default types and scopes, profiles, and config discovery.
//!
//! Precedence, lowest to highest: built-in defaults, then
//! `$XDG_CONFIG_HOME/commit-tui/config.toml`, then `.commit-tui.toml` in the
//! current directory. Lists replace the previous value, they do not merge, so a
//! project file has full control over its own types once it sets them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A commit type: the short name and the one-line explanation shown next to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitType {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub emoji: Option<String>,
}

impl CommitType {
    fn new(name: &str, description: &str, emoji: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            emoji: Some(emoji.to_string()),
        }
    }
}

/// A named set of types, e.g. Angular or conventional commit presets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub types: Vec<CommitType>,
}

/// Everything the composer and the linter need to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Active profile name.
    pub profile: String,
    /// Available profiles, keyed by name.
    pub profiles: BTreeMap<String, Profile>,
    /// Extra scopes offered in the scope picker, beyond the ones from history.
    pub scopes: Vec<String>,
    /// Show emojis in the type picker.
    pub emoji: bool,
    /// Number of commits scanned for scopes.
    pub history_depth: usize,
    /// Hard limit for the subject line.
    pub max_subject: usize,
    /// Softer limit that only produces a warning.
    pub recommended_subject: usize,
    /// Where the values came from, for `commit-tui config`.
    pub sources: Vec<PathBuf>,
}

const ANGULAR: [(&str, &str, &str); 11] = [
    ("feat", "a new feature", "✨"),
    ("fix", "a bug fix", "🐛"),
    ("docs", "documentation only", "📚"),
    ("style", "formatting, no code change", "💎"),
    ("refactor", "code change, no fix or feature", "♻️"),
    ("perf", "performance improvement", "⚡"),
    ("test", "adding or fixing tests", "✅"),
    ("build", "build system or dependencies", "📦"),
    ("ci", "continuous integration", "👷"),
    ("chore", "maintenance, no src change", "🔧"),
    ("revert", "revert a previous commit", "⏪"),
];

const CONVENTIONAL_EXTRA: [(&str, &str, &str); 2] = [
    ("improvement", "improve an existing feature", "🎨"),
    ("security", "security related change", "🔒"),
];

const ATOMIC: [(&str, &str, &str); 11] = [
    ("feat", "adds user visible functionality", "✨"),
    ("fix", "fixes a defect", "🐛"),
    ("perf", "improves speed or memory", "⚡"),
    ("refactor", "restructures without changing behaviour", "♻️"),
    ("docs", "documentation", "📚"),
    ("test", "tests only", "✅"),
    ("build", "build, dependencies, tooling", "📦"),
    ("ci", "pipeline configuration", "👷"),
    ("chore", "housekeeping", "🔧"),
    ("style", "whitespace and formatting", "💎"),
    ("revert", "undoes an earlier commit", "⏪"),
];

impl Config {
    /// Angular types plus the two Conventional Commits extensions.
    pub fn conventional() -> Self {
        Self::with_profiles(vec![
            Profile {
                name: "conventional".into(),
                description: Some("Angular types plus improvement and security".into()),
                types: list(&ANGULAR)
                    .into_iter()
                    .chain(list(&CONVENTIONAL_EXTRA))
                    .collect(),
            },
            Profile {
                name: "angular".into(),
                description: Some("The eleven classic Angular types".into()),
                types: list(&ANGULAR),
            },
            Profile {
                name: "atomic".into(),
                description: Some("Same types, descriptions tuned for small commits".into()),
                types: list(&ATOMIC),
            },
        ])
    }

    fn with_profiles(profiles: Vec<Profile>) -> Self {
        let profiles: BTreeMap<String, Profile> = profiles
            .into_iter()
            .map(|profile| (profile.name.clone(), profile))
            .collect();
        // Alphabetical order would hand out `angular` first; keep the
        // Conventional Commits profile as the default instead.
        Self {
            profile: "conventional".to_string(),
            profiles,
            scopes: Vec::new(),
            emoji: true,
            history_depth: 500,
            max_subject: 72,
            recommended_subject: 50,
            sources: Vec::new(),
        }
    }

    /// The active profile, or an empty one if the name is unknown.
    pub fn active(&self) -> &Profile {
        self.profiles
            .get(&self.profile)
            .or_else(|| self.profiles.values().next())
            .expect("at least one profile")
    }

    pub fn types(&self) -> &[CommitType] {
        &self.active().types
    }

    /// Type names in picker order.
    pub fn type_names(&self) -> Vec<String> {
        self.types().iter().map(|t| t.name.clone()).collect()
    }

    /// Load defaults plus user and project files. A broken file is reported in
    /// the returned warning instead of aborting, so the composer always starts.
    pub fn load(cwd: &Path) -> (Self, Vec<String>) {
        let mut config = Self::conventional();
        let mut warnings = Vec::new();

        for path in Self::candidate_paths(cwd) {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            match toml::from_str::<ConfigFile>(&text) {
                Ok(file) => {
                    config.apply(file);
                    config.sources.push(path);
                }
                Err(error) => warnings.push(format!("ignoring {}: {error}", path.display())),
            }
        }

        (config, warnings)
    }

    /// User config first, then the project file, so the project wins.
    fn candidate_paths(cwd: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            paths.push(PathBuf::from(xdg).join("commit-tui/config.toml"));
        } else if let Ok(home) = std::env::var("HOME") {
            paths.push(PathBuf::from(home).join(".config/commit-tui/config.toml"));
        }
        paths.push(cwd.join(".commit-tui.toml"));
        paths
    }

    fn apply(&mut self, file: ConfigFile) {
        if let Some(profile) = file.profile {
            self.profile = profile;
        }
        if let Some(profiles) = file.profiles {
            self.profiles = profiles
                .into_iter()
                .map(|profile| (profile.name.clone(), profile))
                .collect();
        }
        if let Some(scopes) = file.scopes {
            self.scopes = scopes;
        }
        if let Some(emoji) = file.emoji {
            self.emoji = emoji;
        }
        if let Some(depth) = file.history_depth {
            self.history_depth = depth;
        }
        if let Some(limit) = file.max_subject {
            self.max_subject = limit;
        }
        if let Some(limit) = file.recommended_subject {
            self.recommended_subject = limit;
        }
    }

    /// Serialize the current state as a starting point for a project file.
    pub fn to_toml(&self) -> String {
        let file = ConfigFile {
            profile: Some(self.profile.clone()),
            profiles: Some(self.profiles.values().cloned().collect()),
            scopes: Some(self.scopes.clone()),
            emoji: Some(self.emoji),
            history_depth: Some(self.history_depth),
            max_subject: Some(self.max_subject),
            recommended_subject: Some(self.recommended_subject),
        };
        toml::to_string_pretty(&file).unwrap_or_default()
    }
}

fn list(items: &[(&str, &str, &str)]) -> Vec<CommitType> {
    items
        .iter()
        .map(|(name, description, emoji)| CommitType::new(name, description, emoji))
        .collect()
}

#[derive(Debug, Deserialize, Serialize)]
struct ConfigFile {
    profile: Option<String>,
    profiles: Option<Vec<Profile>>,
    scopes: Option<Vec<String>>,
    emoji: Option<bool>,
    history_depth: Option<usize>,
    max_subject: Option<usize>,
    recommended_subject: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ships_three_profiles() {
        let config = Config::conventional();
        assert_eq!(config.profile, "conventional");
        assert_eq!(config.types().len(), 13);
        assert!(config.profiles.contains_key("angular"));
        assert!(config.profiles.contains_key("atomic"));
    }

    #[test]
    fn active_profile_falls_back_to_first() {
        let mut config = Config::conventional();
        config.profile = "does-not-exist".into();
        assert!(!config.types().is_empty());
    }

    #[test]
    fn toml_round_trip_keeps_types() {
        let config = Config::conventional();
        let text = config.to_toml();
        let file: ConfigFile = toml::from_str(&text).expect("valid toml");
        let profiles = file.profiles.expect("profiles");
        assert_eq!(profiles.len(), 3);
        let by_name = |name: &str| profiles.iter().find(|p| p.name == name).cloned().unwrap();
        assert_eq!(
            by_name(&config.profile).types,
            config.active().types,
            "the active profile survives the round trip"
        );
    }

    #[test]
    fn project_file_overrides_user_file() {
        let dir = std::env::temp_dir().join("commit-tui-config-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".commit-tui.toml"),
            "profile = \"angular\"\nemoji = false\nmax_subject = 60\n",
        )
        .unwrap();
        let (config, warnings) = Config::load(&dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.profile, "angular");
        assert!(!config.emoji);
        assert_eq!(config.max_subject, 60);
        std::fs::remove_dir_all(&dir).ok();
    }
}
