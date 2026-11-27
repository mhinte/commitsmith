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
fn doctor_reports_environment_and_repository() {
    let dir = repo("doctor");
    let output = bin()
        .arg("--doctor")
        .current_dir(&dir)
        .output()
        .expect("run doctor");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    for expected in ["commitsmith ", "os:", "user config:", "repository:", "git:"] {
        assert!(text.contains(expected), "{expected} missing in:\n{text}");
    }
    assert!(
        text.contains(&dir.display().to_string()) || text.contains("repository:  "),
        "the report should name the repository:\n{text}"
    );

    // Outside a repository the same command must still work and say so.
    let empty = std::env::temp_dir().join(format!("commitsmith-plain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    let output = bin()
        .arg("--doctor")
        .current_dir(&empty)
        .output()
        .expect("run doctor");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("repository:  none"), "{text}");
    let _ = std::fs::remove_dir_all(&empty);
    let _ = std::fs::remove_dir_all(&dir);
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

/// Run the installed hook the way git does, with `PATH` pointing at the binary.
fn run_hook(dir: &Path, hooks: &str, message: &Path) -> std::process::Output {
    let binary = Path::new(env!("CARGO_BIN_EXE_commitsmith"));
    Command::new("sh")
        .arg(dir.join(hooks).join("commit-msg"))
        .arg(message)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", binary.parent().unwrap().display()),
        )
        .current_dir(dir)
        .output()
        .expect("run the hook")
}

#[test]
fn a_foreign_hook_is_chained_not_replaced() {
    // E8-2, variant B: the existing hook keeps running, before ours, and its
    // verdict stands. Nothing is lost and nothing needs --force.
    let dir = repo("foreign");
    let hooks = dir.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let foreign = "#!/bin/sh\necho foreign ran >&2\ngrep -q WIP \"$1\" && exit 3\nexit 0\n";
    std::fs::write(hooks.join("commit-msg"), foreign).unwrap();
    make_executable(&hooks.join("commit-msg"));

    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    assert!(
        String::from_utf8_lossy(&install.stdout).contains("runs first"),
        "the user is told: {install:?}"
    );
    assert_eq!(
        std::fs::read_to_string(hooks.join("commit-msg.pre-commitsmith")).unwrap(),
        foreign,
        "the foreign hook is kept byte for byte"
    );

    // Its failure blocks with its own exit code; ours does not even run.
    let message = dir.join("msg.txt");
    std::fs::write(&message, "feat: WIP thing\n").unwrap();
    let blocked = run_hook(&dir, ".git/hooks", &message);
    assert_eq!(blocked.status.code(), Some(3), "{blocked:?}");

    // When it passes, our check decides.
    std::fs::write(&message, "feat:no space\n").unwrap();
    let ours = run_hook(&dir, ".git/hooks", &message);
    assert!(String::from_utf8_lossy(&ours.stderr).contains("foreign ran"));
    assert_eq!(ours.status.code(), Some(1), "our lint blocks: {ours:?}");
    std::fs::write(&message, "feat: a fine message\n").unwrap();
    assert!(run_hook(&dir, ".git/hooks", &message).status.success());

    // Installing again is the update path and keeps the chain intact.
    let again = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(again.status.success(), "{again:?}");
    assert!(hooks.join("commit-msg.pre-commitsmith").exists());

    // Uninstall puts the original back.
    let removed = bin()
        .args(["hooks", "uninstall"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(removed.status.success(), "{removed:?}");
    assert_eq!(
        std::fs::read_to_string(hooks.join("commit-msg")).unwrap(),
        foreign
    );
    assert!(!hooks.join("commit-msg.pre-commitsmith").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_hook_goes_where_core_hooks_path_points() {
    let dir = repo("hooks-path");
    run(&dir, "git", &["config", "core.hooksPath", ".githooks"]);
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    assert!(
        dir.join(".githooks/commit-msg").exists(),
        "where git reads hooks"
    );
    assert!(
        !dir.join(".git/hooks/commit-msg").exists(),
        "not where git ignores them"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_managed_hook_directory_needs_force() {
    // husky regenerates .husky/_ on every install; a hook placed there would
    // vanish without a word, so the user has to ask for it.
    let dir = repo("husky");
    run(&dir, "git", &["config", "core.hooksPath", ".husky/_"]);
    let refused = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!refused.status.success(), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("husky") && stderr.contains("--force"),
        "{stderr}"
    );
    assert!(stderr.lines().all(|line| line.len() < 80), "{stderr}");
    assert!(!dir.join(".husky/_/commit-msg").exists());

    let forced = bin()
        .args(["hooks", "install", "--force"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(forced.status.success(), "{forced:?}");
    assert!(dir.join(".husky/_/commit-msg").exists());

    // A lefthook-generated hook in .git/hooks is recognised by its content.
    let other = repo("lefthook");
    let hook = other.join(".git/hooks/commit-msg");
    std::fs::write(
        &hook,
        "#!/bin/sh\n# generated by lefthook\nlefthook run commit-msg \"$@\"\n",
    )
    .unwrap();
    let refused = bin()
        .args(["hooks", "install"])
        .current_dir(&other)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&refused.stderr).contains("lefthook.yml"));
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(other);
}

#[test]
fn a_hand_written_hook_that_calls_commitsmith_is_not_mistaken_for_ours() {
    // The old marker was the bare word "commitsmith", so this hook used to be
    // overwritten as if it were ours.
    let dir = repo("own-call");
    let hook = dir.join(".git/hooks/commit-msg");
    let mine = "#!/bin/sh\n./scripts/ticket-check \"$1\" && commitsmith check --file \"$1\"\n";
    std::fs::write(&hook, mine).unwrap();
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join(".git/hooks/commit-msg.pre-commitsmith")).unwrap(),
        mine
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

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

#[test]
fn the_hook_does_not_block_a_commit_when_the_binary_is_missing() {
    // A colleague without commitsmith must not be locked out of committing. The
    // hook looks for the binary and steps aside with one line.
    let dir = repo("hook-missing-binary");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let install = bin_with_home(&home)
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    // A colleague's clone does not carry the repo-local `commitsmith.binary`,
    // so the hook has to cope with no usable path at all.
    run(&dir, "git", &["config", "--unset", "commitsmith.binary"]); // may be absent
    let message = dir.join("msg.txt");
    std::fs::write(&message, "feat:no space after the colon\n").unwrap();
    // PATH without the binary, HOME without a config: nothing to find.
    let hook = Command::new("sh")
        .arg(dir.join(".git/hooks/commit-msg"))
        .arg(&message)
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        hook.status.success(),
        "the commit must go through: {hook:?}"
    );
    let stderr = String::from_utf8_lossy(&hook.stderr);
    assert!(stderr.contains("commitsmith not found"), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "one line, no noise: {stderr}");

    // A stored path that has moved away (upgrade, home moved, a colleague's
    // machine) must be ignored, not trusted.
    run(
        &dir,
        "git",
        &["config", "commitsmith.binary", "/nonexistent/commitsmith"],
    );
    let stale = Command::new("sh")
        .arg(dir.join(".git/hooks/commit-msg"))
        .arg(&message)
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        stale.status.success(),
        "a stale path must not block the commit: {stale:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_hook_blocks_a_bad_message_when_the_binary_is_there() {
    let dir = repo("hook-with-binary");
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let message = dir.join("msg.txt");
    std::fs::write(&message, "feat:no space after the colon\n").unwrap();
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_commitsmith"));
    let path = format!("{}:/usr/bin:/bin", binary.parent().unwrap().display());
    let hook = Command::new("sh")
        .arg(dir.join(".git/hooks/commit-msg"))
        .arg(&message)
        .env("PATH", &path)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        !hook.status.success(),
        "a violation must still block: {hook:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_configured_binary_wins_over_the_path() {
    // `git config commitsmith.binary` is the answer for an unusual install path
    // (and for Windows, where the GUI PATH differs from the terminal's).
    let dir = repo("hook-configured-binary");
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    run(
        &dir,
        "git",
        &[
            "config",
            "commitsmith.binary",
            env!("CARGO_BIN_EXE_commitsmith"),
        ],
    );

    let message = dir.join("msg.txt");
    std::fs::write(&message, "feat:no space after the colon\n").unwrap();
    // PATH without the binary: only the config can find it.
    let hook = Command::new("sh")
        .arg(dir.join(".git/hooks/commit-msg"))
        .arg(&message)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        !hook.status.success(),
        "the configured binary was used: {hook:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn installing_the_hook_remembers_where_the_binary_lives() {
    // The hook asks `git config commitsmith.binary` first; install fills it in,
    // so the risky install paths (zip in ~/tools, portable build) work without
    // the user knowing about the setting.
    let dir = repo("hook-remembers-binary");
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    let stored = String::from_utf8_lossy(
        &run(&dir, "git", &["config", "--get", "commitsmith.binary"]).stdout,
    )
    .to_string();
    assert_eq!(
        stored.trim(),
        env!("CARGO_BIN_EXE_commitsmith"),
        "the running binary is recorded"
    );

    // Installing again keeps it working (the update path).
    let again = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(again.status.success(), "{again:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn installing_writes_both_hooks() {
    let dir = repo("both-hooks");
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    for hook in ["commit-msg", "prepare-commit-msg"] {
        let path = dir.join(".git/hooks").join(hook);
        assert!(path.exists(), "{hook} installed: {install:?}");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("# commitsmith"),
            "{hook} carries the marker"
        );
    }
    let removed = bin()
        .args(["hooks", "uninstall"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(removed.status.success(), "{removed:?}");
    for hook in ["commit-msg", "prepare-commit-msg"] {
        assert!(
            !dir.join(".git/hooks").join(hook).exists(),
            "{hook} removed"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Run one installed hook the way git does: `sh <hook> <file> [<source>]`.
fn run_named_hook(
    dir: &Path,
    name: &str,
    file: &Path,
    source: Option<&str>,
) -> std::process::Output {
    let binary = Path::new(env!("CARGO_BIN_EXE_commitsmith"));
    let mut command = Command::new("sh");
    command
        .arg(dir.join(".git/hooks").join(name))
        .arg(file)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", binary.parent().unwrap().display()),
        )
        .current_dir(dir);
    if let Some(source) = source {
        command.arg(source);
    }
    command.output().expect("run the hook")
}

#[test]
fn the_prepare_hook_starts_an_empty_message_from_the_branch() {
    let dir = repo("prepare-empty");
    run(
        &dir,
        "git",
        &["checkout", "-q", "-b", "feat/hima/TICKET-12/add-a-thing"],
    );
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    // What git leaves in the file: comments only.
    let file = dir.join(".git/COMMIT_EDITMSG");
    std::fs::write(&file, "# Please enter the commit message\n#\n").unwrap();
    let hook = run_named_hook(&dir, "prepare-commit-msg", &file, None);
    assert!(hook.status.success(), "{hook:?}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "feat(TICKET-12): \n# Please enter the commit message\n#\n"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_prepare_hook_never_touches_a_message_git_brought_along() {
    // merge, squash, --amend, -m and -F all pass a source in $2.
    let dir = repo("prepare-source");
    run(&dir, "git", &["checkout", "-q", "-b", "fix/hima/PROJ-7"]);
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    let file = dir.join(".git/COMMIT_EDITMSG");
    for source in ["merge", "squash", "commit", "message", "template"] {
        std::fs::write(&file, "# comment\n").unwrap();
        let hook = run_named_hook(&dir, "prepare-commit-msg", &file, Some(source));
        assert!(hook.status.success(), "{source}: {hook:?}");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "# comment\n",
            "{source} leaves the file alone"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_prepare_hook_keeps_what_the_writer_typed() {
    let dir = repo("prepare-written");
    run(
        &dir,
        "git",
        &["checkout", "-q", "-b", "docs/hima/TICKET-5/readme"],
    );
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    let file = dir.join(".git/COMMIT_EDITMSG");
    std::fs::write(&file, "fix: my own line\n\nand a body\n").unwrap();
    let hook = run_named_hook(&dir, "prepare-commit-msg", &file, None);
    assert!(hook.status.success(), "{hook:?}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "fix: my own line\n\nand a body\n",
        "no prefix over writing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_prepare_hook_is_silent_and_harmless_without_a_binary() {
    let dir = repo("prepare-no-binary");
    run(
        &dir,
        "git",
        &["checkout", "-q", "-b", "feat/hima/TICKET-1/x"],
    );
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let install = bin_with_home(&home)
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    run(&dir, "git", &["config", "--unset", "commitsmith.binary"]);

    let file = dir.join(".git/COMMIT_EDITMSG");
    std::fs::write(&file, "# comment\n").unwrap();
    let hook = Command::new("sh")
        .arg(dir.join(".git/hooks/prepare-commit-msg"))
        .arg(&file)
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(hook.status.success(), "{hook:?}");
    assert!(hook.stdout.is_empty() && hook.stderr.is_empty(), "{hook:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "# comment\n");
    let _ = std::fs::remove_dir_all(dir);
}
