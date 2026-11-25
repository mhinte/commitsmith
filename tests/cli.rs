//! End-to-end checks against a throwaway git repository.
//!
//! These drive the real binary, the way CI does, instead of calling into the
//! modules: argument parsing, exit codes and the installed hook are part of what
//! ships and are not covered by unit tests.

use std::path::Path;
use std::process::Command;

/// Create an empty repository in a fresh temporary directory.
fn repo(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("commit-tui-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    run(&dir, "git", &["init", "-q"]);
    run(&dir, "git", &["config", "user.email", "ci@example.com"]);
    run(&dir, "git", &["config", "user.name", "CI"]);
    dir
}

fn run(dir: &Path, program: &str, args: &[&str]) -> std::process::Output {
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|error| panic!("cannot run {program}: {error}"))
}

/// The binary under test.
fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_commitsmith"))
}

/// A command with `HOME` (and the XDG variables) pointed at `home`, so the test
/// cannot read or write the developer's real config.
fn bin_with_home(home: &Path) -> Command {
    let mut command = bin();
    command
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env_remove("APPDATA")
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"));
    command
}

fn check(dir: &Path, message: &str) -> std::process::Output {
    bin()
        .args(["check", "--message", message])
        .current_dir(dir)
        .output()
        .expect("run check")
}

fn add_file(dir: &Path, name: &str) {
    std::fs::write(dir.join(name), "content\n").expect("write file");
    run(dir, "git", &["add", name]);
}

#[test]
fn init_defaults_writes_a_usable_config_and_is_idempotent() {
    let home = std::env::temp_dir().join(format!("commitsmith-init-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    let first = bin_with_home(&home)
        .args(["init", "--defaults"])
        .current_dir(&home)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let path = home.join(".config/commitsmith/config.toml");
    let text = std::fs::read_to_string(&path).expect("config written");
    assert!(text.contains("author = \"mmu\""), "{text}");
    assert!(text.contains("ticket_prefix = \"TICKET\""), "{text}");
    assert!(text.contains("reference = \"Refs\""), "{text}");
    assert!(text.contains("branch_template"), "{text}");

    // The values are readable again, and a second run asks for --force.
    let config = bin_with_home(&home)
        .args(["config"])
        .current_dir(&home)
        .output()
        .unwrap();
    assert!(config.status.success());
    let shown = String::from_utf8_lossy(&config.stdout);
    assert!(shown.contains("TICKET"), "{shown}");

    let second = bin_with_home(&home)
        .args(["init", "--defaults"])
        .current_dir(&home)
        .output()
        .unwrap();
    assert!(
        !second.status.success(),
        "second run must refuse without --force"
    );
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, after, "refused run must not touch the file");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn accepts_conventional_and_rejects_plain_messages() {
    let dir = repo("check");
    let good = check(&dir, "feat(api): add rate limit");
    assert!(good.status.success(), "{good:?}");

    let bad = check(&dir, "Add stuff.");
    assert!(!bad.status.success());
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(stderr.contains("error"), "{stderr}");
}

#[test]
fn strict_fails_on_warnings_only() {
    let dir = repo("strict");
    let long = "feat: ".to_string() + &"a".repeat(60);
    let plain = check(&dir, &long);
    assert!(plain.status.success(), "warnings alone must pass");

    let strict = bin()
        .args(["check", "--strict", "--message", &long])
        .current_dir(&dir)
        .output()
        .expect("run check");
    assert!(!strict.status.success(), "strict must fail on warnings");
}

#[test]
fn hook_blocks_a_bad_message_and_then_lets_a_good_one_through() {
    let dir = repo("hook");
    add_file(&dir, "a.txt");

    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .expect("run hooks install");
    assert!(install.status.success(), "{install:?}");
    assert!(dir.join(".git/hooks/commit-msg").exists());

    // The hook calls `commit-tui` by name, so put the binary under test on PATH.
    let bin_dir = std::path::Path::new(env!("CARGO_BIN_EXE_commitsmith"))
        .parent()
        .expect("binary has a parent dir");
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let bad = Command::new("git")
        .args(["commit", "-m", "Add stuff."])
        .current_dir(&dir)
        .env("PATH", &path)
        .output()
        .expect("git commit");
    assert!(
        !bad.status.success(),
        "the hook let a plain message through"
    );

    let good = Command::new("git")
        .args(["commit", "-m", "feat(hook): prove the hook works"])
        .current_dir(&dir)
        .env("PATH", &path)
        .output()
        .expect("git commit");
    assert!(good.status.success(), "{good:?}");

    let log = run(&dir, "git", &["log", "-1", "--format=%s"]);
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "feat(hook): prove the hook works"
    );

    let uninstall = bin()
        .args(["hooks", "uninstall"])
        .current_dir(&dir)
        .output()
        .expect("run hooks uninstall");
    assert!(uninstall.status.success(), "{uninstall:?}");
    assert!(!dir.join(".git/hooks/commit-msg").exists());
}

#[test]
fn foreign_hooks_are_not_overwritten_without_force() {
    let dir = repo("foreign");
    let hook = dir.join(".git/hooks/commit-msg");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();

    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .expect("run hooks install");
    assert!(!install.status.success(), "must refuse to clobber");
    assert!(String::from_utf8_lossy(&install.stderr).contains("--force"));
    let content = std::fs::read_to_string(&hook).unwrap();
    assert_eq!(
        content, "#!/bin/sh\nexit 0\n",
        "the foreign hook is untouched"
    );

    let forced = bin()
        .args(["hooks", "install", "--force"])
        .current_dir(&dir)
        .output()
        .expect("run hooks install --force");
    assert!(forced.status.success(), "{forced:?}");
    assert!(
        dir.join(".git/hooks/commit-msg.bak").exists(),
        "backup written"
    );
    assert!(
        std::fs::read_to_string(&hook)
            .unwrap()
            .contains("commitsmith")
    );
}

#[test]
fn user_flag_writes_the_identity_file() {
    let dir = repo("identity");
    let config_home = dir.join("xdg");

    let output = bin()
        .args(["--user", "Marc Hinte"])
        .current_dir(&dir)
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .expect("run --user");
    assert!(output.status.success(), "{output:?}");

    let text = std::fs::read_to_string(config_home.join("commitsmith/config.toml"))
        .expect("identity file written");
    assert!(text.contains("user = \"Marc Hinte\""), "{text}");
    assert!(text.contains("prefix = \"himar\""), "{text}");

    let shown = bin()
        .args(["config", "--identity"])
        .current_dir(&dir)
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .expect("run config --identity");
    let stdout = String::from_utf8_lossy(&shown.stdout);
    assert!(stdout.contains("Marc Hinte"), "{stdout}");
    assert!(stdout.contains("himar"), "{stdout}");
}

#[test]
fn unknown_command_fails_with_usage() {
    let output = bin().arg("nonsense").output().expect("run binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage") || stderr.contains("unrecognized"),
        "{stderr}"
    );
}
