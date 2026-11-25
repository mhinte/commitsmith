//! Default types and scopes, profiles, and config discovery.
//!
//! Precedence, lowest to highest: built-in defaults, then
//! `$XDG_CONFIG_HOME/commitsmith/config.toml`, then `.commitsmith.toml` in the
//! current directory. Lists replace the previous value, they do not merge, so a
//! project file has full control over its own types once it sets them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::errors::{ConfigError, ConfigScope};
use crate::domain::ports::{ConfigDocument, ConfigStore};

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
    /// Name the user entered on first start, if any.
    pub user: Option<String>,
    /// Short handle derived from the name, e.g. `hima`.
    pub prefix: Option<String>,
    /// Author shortcut for branch names, e.g. `mmu`.
    pub author: Option<String>,
    /// Prefix for ticket ids, e.g. `TICKET`.
    pub ticket_prefix: String,
    /// `Refs` or `Closes`, the keyword the footer uses.
    pub reference: String,
    /// Template for branch names.
    pub branch_template: String,
    /// Add `Co-authored-by: <name>` to a new message's footer.
    pub co_author: bool,
    /// Hide body, breaking and footer until they are needed.
    pub compact_form: bool,
    /// Text after `Co-authored-by: `; falls back to the handle from setup.
    pub co_author_name: Option<String>,
    /// Where the values came from, for `commitsmith config`.
    pub sources: Vec<PathBuf>,
}

/// Handle from a full name: first two letters of the surname, first three of
/// the given name, lower case. `Marc Hinte` becomes `hima`.
///
/// German conventions are expected: the surname is the last word. A single word
/// simply yields its first three letters, and umlauts are spelled out so the
/// handle stays ASCII (`Müller` -> `mu`).
pub fn prefix_from_name(full_name: &str) -> Option<String> {
    let words: Vec<&str> = full_name.split_whitespace().collect();
    // Take letters before folding umlauts: "ü" is one letter, "ue" two.
    let letters = |word: &str, count: usize| -> String {
        let raw: String = word
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(count)
            .collect();
        transliterate(&raw)
    };

    match words.len() {
        0 => None,
        1 => {
            let only = letters(words[0], 3);
            if only.is_empty() { None } else { Some(only) }
        }
        _ => {
            // "hi" of Hinte, "mar" of Marc.
            let surname = letters(words[words.len() - 1], 2);
            let given = letters(words[0], 3);
            let prefix = format!("{surname}{given}");
            if prefix.is_empty() {
                None
            } else {
                Some(prefix)
            }
        }
    }
}

/// Lower-case ASCII form of `word`, umlauts and accents spelled out.
fn transliterate(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars() {
        match c {
            'ä' | 'Ä' => out.push_str("ae"),
            'ö' | 'Ö' => out.push_str("oe"),
            'ü' | 'Ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            // Accented latin letters: keep the base letter.
            c if c.is_alphanumeric() => {
                if let Some(base) = c
                    .to_lowercase()
                    .next()
                    .filter(|b| b.is_ascii_alphanumeric())
                {
                    out.push(base);
                }
            }
            _ => {}
        }
    }
    out
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
            user: None,
            prefix: None,
            author: None,
            ticket_prefix: "TICKET".to_string(),
            reference: "Refs".to_string(),
            co_author: true,
            compact_form: false,
            co_author_name: None,
            branch_template: "{type}/{author}/{ticket}/{slug}".to_string(),
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
    ///
    /// The two earlier names (`commit-tui`, `coggit`) are still read, so an
    /// existing setup keeps working; `.commitsmith.toml` wins when both exist.
    fn candidate_paths(cwd: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(config_dir) = Self::user_config_dir() {
            paths.push(config_dir.join("commit-tui/config.toml"));
            paths.push(config_dir.join("coggit/config.toml"));
            paths.push(config_dir.join("commitsmith/config.toml"));
        }
        paths.push(cwd.join(".commit-tui.toml"));
        paths.push(cwd.join(".coggit.toml"));
        paths.push(cwd.join(".commitsmith.toml"));
        paths
    }

    /// `$XDG_CONFIG_HOME`, else `%APPDATA%` on Windows, else `~/.config`.
    fn user_config_dir() -> Option<PathBuf> {
        config_dir_from(
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
            std::env::var("APPDATA").ok().as_deref(),
            std::env::var("HOME").ok().as_deref(),
            std::env::var("USERPROFILE").ok().as_deref(),
        )
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
        // A file that names a user but no prefix means "derive it": a
        // hand-written file looks like that, and so does one from an older
        // version that stored the handle under a different name.
        let explicit_prefix = file
            .prefix
            .filter(|prefix| !prefix.trim().is_empty())
            .map(|prefix| prefix.trim().to_lowercase());
        let named_user = file.user.filter(|name| !name.trim().is_empty());

        if let Some(user) = named_user.clone() {
            self.user = Some(user);
        }
        if let Some(author) = file.author.filter(|value| !value.trim().is_empty()) {
            self.author = Some(author.trim().to_lowercase());
        }
        if let Some(prefix) = file.ticket_prefix.filter(|value| !value.trim().is_empty()) {
            self.ticket_prefix = prefix.trim().to_uppercase();
        }
        if let Some(reference) = file.reference.filter(|value| !value.trim().is_empty()) {
            self.reference = reference.trim().to_string();
        }
        if let Some(template) = file
            .branch_template
            .filter(|value| !value.trim().is_empty())
        {
            self.branch_template = template.trim().to_string();
        }
        if let Some(co_author) = file.co_author {
            self.co_author = co_author;
        }
        if let Some(compact) = file.compact_form {
            self.compact_form = compact;
        }
        if let Some(name) = file.co_author_name {
            // An empty string is a deliberate "no text beyond the keyword".
            self.co_author_name = Some(name.trim().to_string()).filter(|value| !value.is_empty());
        }
        if let Some(prefix) = explicit_prefix {
            self.prefix = Some(prefix);
        } else if named_user.is_some() {
            self.prefix = self.user.as_deref().and_then(prefix_from_name);
        }
    }

    /// Path of the user-wide config file, creating nothing.
    pub fn user_config_path() -> Option<PathBuf> {
        Self::user_config_dir().map(|dir| dir.join("commitsmith/config.toml"))
    }

    /// True when no user config exists yet, i.e. this is a first start.
    pub fn needs_setup() -> bool {
        Self::user_config_path().is_none_or(|path| !path.exists())
    }

    /// Write the user-wide config with the given name and derived handle.
    ///
    /// Only what the user typed is stored: the rest keeps following the built-in
    /// defaults and any project file, so later upgrades are not frozen in.
    pub fn save_identity(full_name: &str) -> Result<PathBuf, String> {
        Self::identity_path().and_then(|path| Self::save_identity_at(full_name, &path))
    }

    /// Where the identity goes, even if the file already exists.
    pub fn identity_path() -> Result<PathBuf, String> {
        Self::user_config_path().ok_or_else(|| {
            if cfg!(windows) {
                "cannot work out the config directory: APPDATA and USERPROFILE are both unset"
                    .to_string()
            } else {
                "cannot work out the config directory: XDG_CONFIG_HOME and HOME are both unset"
                    .to_string()
            }
        })
    }

    /// Same as [`save_identity`](Self::save_identity), for a caller that picked
    /// the path itself (the CLI takes an explicit `--user`).
    pub fn save_identity_at(full_name: &str, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let identity = IdentityFile {
            user: Some(full_name.trim().to_string()),
            prefix: prefix_from_name(full_name),
        };
        let text = toml::to_string_pretty(&identity)
            .map_err(|error| format!("cannot serialize the config: {error}"))?;
        let header = concat!(
            "# Written on first start by commitsmith.\n",
            "# `prefix` is derived from `user`: first two letters of the surname,\n",
            "# then the first three of the given name. Edit both freely.\n",
        );
        std::fs::write(path, format!("{header}{text}"))
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        Ok(path.to_path_buf())
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
            user: self.user.clone(),
            prefix: self.prefix.clone(),
            author: self.author.clone(),
            ticket_prefix: Some(self.ticket_prefix.clone()),
            reference: Some(self.reference.clone()),
            branch_template: Some(self.branch_template.clone()),
            co_author: Some(self.co_author),
            compact_form: Some(self.compact_form),
            co_author_name: self.co_author_name.clone(),
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

/// The part of the config that the first-start dialog writes.
#[derive(Debug, Deserialize, Serialize)]
struct IdentityFile {
    user: Option<String>,
    prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ConfigFile {
    profile: Option<String>,
    profiles: Option<Vec<Profile>>,
    author: Option<String>,
    ticket_prefix: Option<String>,
    reference: Option<String>,
    branch_template: Option<String>,
    co_author: Option<bool>,
    compact_form: Option<bool>,
    co_author_name: Option<String>,
    scopes: Option<Vec<String>>,
    emoji: Option<bool>,
    history_depth: Option<usize>,
    max_subject: Option<usize>,
    recommended_subject: Option<usize>,
    user: Option<String>,
    prefix: Option<String>,
}

/// Pick the directory the user config lives in.
///
/// Windows has neither `XDG_CONFIG_HOME` nor, outside a POSIX shell, `HOME`; the
/// roaming app data directory is the equivalent there. An empty variable counts
/// as unset, because that is how some shells and CI runners pass them on.
fn config_dir_from(
    xdg: Option<&str>,
    appdata: Option<&str>,
    home: Option<&str>,
    userprofile: Option<&str>,
) -> Option<PathBuf> {
    config_dir_for(cfg!(windows), xdg, appdata, home, userprofile)
}

/// The rule itself, with the platform as a parameter so both branches are
/// testable on any machine.
fn config_dir_for(
    windows: bool,
    xdg: Option<&str>,
    appdata: Option<&str>,
    home: Option<&str>,
    userprofile: Option<&str>,
) -> Option<PathBuf> {
    fn present(value: Option<&str>) -> Option<&str> {
        value.filter(|text| !text.trim().is_empty())
    }
    if let Some(dir) = present(xdg) {
        return Some(PathBuf::from(dir));
    }
    if windows {
        if let Some(dir) = present(appdata) {
            return Some(PathBuf::from(dir));
        }
        // Last resort on Windows: the roaming folder under the profile.
        if let Some(dir) = present(userprofile) {
            return Some(PathBuf::from(dir).join("AppData").join("Roaming"));
        }
    }
    present(home).map(|dir| PathBuf::from(dir).join(".config"))
}

/// The TOML files behind [`ConfigStore`](crate::domain::ports::ConfigStore).
///
/// Reads return the document verbatim; `Config` stays the one place that knows
/// what the keys mean. Writes are atomic: a temp file in the same directory plus
/// a rename, so an interrupted save cannot leave half a config behind.
pub struct TomlConfigStore {
    /// Set by tests so they do not have to touch the process environment.
    user_dir: Option<PathBuf>,
}

impl TomlConfigStore {
    pub fn new() -> Self {
        Self { user_dir: None }
    }

    /// A store rooted at `dir` instead of the platform config directory.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            user_dir: Some(dir.into()),
        }
    }

    fn user_path(&self) -> Result<PathBuf, ConfigError> {
        if let Some(dir) = &self.user_dir {
            return Ok(dir.join("commitsmith").join("config.toml"));
        }
        Config::user_config_path().ok_or(ConfigError::not_found(ConfigScope::User))
    }
}

/// Store the wizard's answers, keeping whatever the file already had.
///
/// Written as TOML values rather than by formatting strings, so a key with a
/// quote or a backslash in it cannot break the file.
pub fn save_setup(
    store: &dyn ConfigStore,
    answers: &crate::domain::setup::SetupAnswers,
) -> Result<PathBuf, ConfigError> {
    let existing = store.user_document()?;
    let mut table: toml::Table = match existing.as_ref().map(|doc| doc.text.trim()) {
        Some(text) if !text.is_empty() => text.parse().map_err(|error| {
            ConfigError::parse(
                existing
                    .as_ref()
                    .map(|doc| doc.path.display().to_string())
                    .unwrap_or_else(|| "user config".to_string()),
                format!("{error}"),
            )
        })?,
        _ => toml::Table::new(),
    };

    table.insert("author".into(), answers.author.as_str().into());
    table.insert("ticket_prefix".into(), answers.ticket_prefix.clone().into());
    table.insert("reference".into(), answers.reference.to_string().into());
    table.insert(
        "branch_template".into(),
        answers.branch_template.as_str().to_string().into(),
    );
    table.insert("profile".into(), answers.profile.clone().into());
    table.insert("emoji".into(), answers.emoji.into());
    table.insert("co_author".into(), answers.co_author.into());

    let text = toml::to_string_pretty(&table)
        .map_err(|error| ConfigError::parse("user config", format!("{error}")))?;
    store.write_user(&text)
}

impl Default for TomlConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigStore for TomlConfigStore {
    fn user_document(&self) -> Result<Option<ConfigDocument>, ConfigError> {
        let path = self.user_path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(ConfigDocument { path, text })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ConfigError::io(
                format!("reading {}", path.display()),
                error,
            )),
        }
    }

    fn project_document(&self, start: &Path) -> Result<Option<ConfigDocument>, ConfigError> {
        let mut dir = Some(start);
        while let Some(current) = dir {
            for name in [".commitsmith.toml", ".coggit.toml", ".commit-tui.toml"] {
                let candidate = current.join(name);
                if candidate.is_file() {
                    let text = std::fs::read_to_string(&candidate).map_err(|error| {
                        ConfigError::io(format!("reading {}", candidate.display()), error)
                    })?;
                    return Ok(Some(ConfigDocument {
                        path: candidate,
                        text,
                    }));
                }
            }
            dir = current.parent();
        }
        Ok(None)
    }

    fn write_user(&self, text: &str) -> Result<PathBuf, ConfigError> {
        let path = self.user_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ConfigError::io(format!("creating {}", parent.display()), error)
            })?;
        }
        let temp = path.with_extension("toml.tmp");
        std::fs::write(&temp, text)
            .map_err(|error| ConfigError::io(format!("writing {}", temp.display()), error))?;
        std::fs::rename(&temp, &path).map_err(|error| {
            let _ = std::fs::remove_file(&temp);
            ConfigError::io(format!("replacing {}", path.display()), error)
        })?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-config-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_co_author_can_be_switched_off_in_the_file() {
        let mut config = Config::conventional();
        config.apply(toml::from_str("user = \"Marc Hinte\"\nco_author = false\n").unwrap());
        assert!(!config.co_author, "false in the file must stay false");
        assert_eq!(config.prefix.as_deref(), Some("himar"));
    }

    #[test]
    fn a_co_author_name_replaces_the_handle() {
        let mut config = Config::conventional();
        config.apply(
            toml::from_str(
                "user = \"Marc Hinte\"\nco_author_name = \"Marc Hinte <marc@example.com>\"\n",
            )
            .unwrap(),
        );
        assert_eq!(
            config.co_author_name.as_deref(),
            Some("Marc Hinte <marc@example.com>")
        );
    }

    #[test]
    fn the_compact_form_setting_survives_a_round_trip() {
        let mut config = Config::conventional();
        assert!(!config.compact_form, "off by default");
        config.compact_form = true;
        let text = config.to_toml();
        assert!(text.contains("compact_form = true"), "{text}");

        let mut reloaded = Config::conventional();
        reloaded.apply(toml::from_str(&text).unwrap());
        assert!(reloaded.compact_form, "the file wins over the default");
    }

    #[test]
    fn the_co_author_setting_survives_a_round_trip() {
        let mut config = Config::conventional();
        config.co_author = false;
        config.co_author_name = Some("Nobody <nobody@example.com>".to_string());
        let text = config.to_toml();
        assert!(text.contains("co_author = false"), "{text}");
        assert!(text.contains("nobody@example.com"), "{text}");
    }

    #[test]
    fn a_missing_user_file_is_none_not_an_error() {
        let dir = temp("missing");
        let store = TomlConfigStore::at(&dir);
        assert_eq!(store.user_document().unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_user_is_atomic_and_readable() {
        let dir = temp("write");
        let store = TomlConfigStore::at(&dir);
        let path = store.write_user("user = \"Marc Hinte\"\n").unwrap();
        assert!(path.ends_with("commitsmith/config.toml"));
        let document = store.user_document().unwrap().unwrap();
        assert!(document.text.contains("Marc Hinte"));
        // No temp file left behind.
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_setup_keeps_the_identity_that_is_already_there() {
        let dir = temp("merge");
        let store = TomlConfigStore::at(&dir);
        store
            .write_user("user = \"Marc Hinte\"\nprefix = \"himar\"\n")
            .unwrap();
        let answers = crate::domain::setup::SetupAnswers::defaults(&["conventional".to_string()])
            .with_author("mhi")
            .unwrap()
            .with_ticket_prefix("abc")
            .unwrap();
        save_setup(&store, &answers).unwrap();

        let text = store.user_document().unwrap().unwrap().text;
        assert!(text.contains("Marc Hinte"), "{text}");
        assert!(text.contains("himar"), "{text}");
        assert!(text.contains("author = \"mhi\""), "{text}");
        assert!(text.contains("ticket_prefix = \"ABC\""), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_project_document_is_found_upwards() {
        let dir = temp("project");
        let nested = dir.join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join(".commitsmith.toml"), "profile = \"angular\"\n").unwrap();
        let store = TomlConfigStore::new();
        let document = store.project_document(&nested).unwrap().unwrap();
        assert!(document.text.contains("angular"));
        assert_eq!(document.path, dir.join(".commitsmith.toml"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn without_a_project_file_there_is_none() {
        let dir = temp("noproject");
        let store = TomlConfigStore::new();
        assert_eq!(store.project_document(&dir).unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn windows_uses_appdata_when_there_is_no_xdg_or_home() {
        // Exactly the input that made the first-start dialog fail on Windows:
        // no XDG_CONFIG_HOME, no HOME, but APPDATA is there.
        let dir = config_dir_for(
            true,
            None,
            Some("C:\\Users\\marc\\AppData\\Roaming"),
            None,
            Some("C:\\Users\\marc"),
        )
        .expect("APPDATA is enough on Windows");
        assert_eq!(dir, PathBuf::from("C:\\Users\\marc\\AppData\\Roaming"));
    }

    #[test]
    fn windows_falls_back_to_the_profile_directory() {
        let dir = config_dir_for(true, None, None, None, Some("C:\\Users\\marc")).unwrap();
        // join() uses the host separator, so compare the structure, not the slash.
        let text = dir.to_string_lossy().replace('\\', "/");
        assert_eq!(text, "C:/Users/marc/AppData/Roaming");
    }

    #[test]
    fn empty_variables_count_as_unset() {
        assert!(config_dir_for(true, Some(""), Some("   "), None, None).is_none());
        assert!(config_dir_for(false, None, None, Some(""), None).is_none());
    }

    #[test]
    fn xdg_wins_over_everything() {
        let dir = config_dir_for(
            true,
            Some("/tmp/cfg"),
            Some("/app"),
            Some("/home/marc"),
            None,
        );
        assert_eq!(dir, Some(PathBuf::from("/tmp/cfg")));
    }

    #[test]
    fn unix_still_uses_home_dot_config() {
        let dir = config_dir_for(false, None, Some("/app"), Some("/home/marc"), None);
        assert_eq!(dir, Some(PathBuf::from("/home/marc/.config")));
    }

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
    fn prefix_uses_two_letters_of_the_surname_and_three_of_the_given_name() {
        assert_eq!(prefix_from_name("Marc Hinte").as_deref(), Some("himar"));
        assert_eq!(
            prefix_from_name("  marc   HINTE  ").as_deref(),
            Some("himar")
        );
        assert_eq!(
            prefix_from_name("Anna Maria Schmidt").as_deref(),
            Some("scann")
        );
        assert_eq!(prefix_from_name("Max").as_deref(), Some("max"));
        assert_eq!(prefix_from_name("Jörg Müller").as_deref(), Some("muejoer"));
        assert_eq!(prefix_from_name("Zoë Weiß").as_deref(), Some("wezo"));
        assert_eq!(prefix_from_name("Ost Apotheke").as_deref(), Some("apost"));
        assert_eq!(prefix_from_name("   ").as_deref(), None);
        assert_eq!(prefix_from_name("").as_deref(), None);
    }

    #[test]
    fn identity_file_round_trips_through_load() {
        let dir = std::env::temp_dir().join(format!("commitsmith-identity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        Config::save_identity_at("Marc Hinte", &path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("user = \"Marc Hinte\""), "{text}");
        assert!(text.contains("prefix = \"himar\""), "{text}");

        let file: ConfigFile = toml::from_str(&text).expect("valid toml");
        assert_eq!(file.user.as_deref(), Some("Marc Hinte"));
        assert_eq!(file.prefix.as_deref(), Some("himar"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_newer_file_overrides_an_older_handle() {
        // Reproduces the upgrade path: an old config with a handle, then a new
        // file that only names the user.
        let mut config = Config::conventional();
        config.apply(
            toml::from_str::<ConfigFile>("user = \"Alt Besitzer\"\nprefix = \"bealt\"\n")
                .expect("valid toml"),
        );
        assert_eq!(config.prefix.as_deref(), Some("bealt"));
        config.apply(toml::from_str::<ConfigFile>("user = \"Marc Hinte\"\n").expect("valid toml"));
        assert_eq!(config.prefix.as_deref(), Some("himar"));
    }

    #[test]
    fn a_name_without_prefix_gets_one_derived() {
        let mut config = Config::conventional();
        config.apply(toml::from_str::<ConfigFile>("user = \"Marc Hinte\"\n").expect("valid toml"));
        assert_eq!(config.prefix.as_deref(), Some("himar"));
    }

    #[test]
    fn project_file_overrides_user_file() {
        let dir = std::env::temp_dir().join("commitsmith-config-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".commitsmith.toml"),
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
