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
fn installing_writes_all_three_hooks() {
    let dir = repo("both-hooks");
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    for hook in [
        "commit-msg",
        "prepare-commit-msg",
        "pre-commit",
        "post-checkout",
    ] {
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
    // Conventional Commits: the type in the header, the ticket in the footer —
    // a human editor is the only case where the hook writes anything at all.
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "feat: \n# Please enter the commit message\n#\n"
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

#[test]
fn the_hints_say_nothing_about_a_normal_index() {
    let dir = repo("hints-quiet");
    std::fs::write(dir.join("one.txt"), "x").unwrap();
    run(&dir, "git", &["add", "one.txt"]);
    let hints = bin().args(["hints"]).current_dir(&dir).output().unwrap();
    assert!(hints.status.success(), "{hints:?}");
    assert!(
        hints.stdout.is_empty() && hints.stderr.is_empty(),
        "silence is the normal case: {hints:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_hints_mention_a_large_index_and_never_block() {
    let dir = repo("hints-many");
    for index in 0..26 {
        std::fs::write(dir.join(format!("f{index}.txt")), "x").unwrap();
    }
    run(&dir, "git", &["add", "."]);
    let hints = bin().args(["hints"]).current_dir(&dir).output().unwrap();
    assert_eq!(hints.status.code(), Some(0), "never a verdict: {hints:?}");
    let stderr = String::from_utf8_lossy(&hints.stderr);
    assert!(stderr.contains("26 files staged"), "{stderr}");
    assert!(stderr.lines().all(|line| line.len() < 80), "{stderr}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_pre_commit_hook_warns_and_passes_the_verdict_of_a_foreign_hook() {
    let dir = repo("pre-commit-chain");
    let hooks = dir.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let foreign = "#!/bin/sh\necho foreign pre-commit ran >&2\nexit 0\n";
    std::fs::write(hooks.join("pre-commit"), foreign).unwrap();
    make_executable(&hooks.join("pre-commit"));

    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    assert!(hooks.join("pre-commit.pre-commitsmith").exists(), "chained");
    for index in 0..26 {
        std::fs::write(dir.join(format!("f{index}.txt")), "x").unwrap();
    }
    run(&dir, "git", &["add", "."]);

    let binary = Path::new(env!("CARGO_BIN_EXE_commitsmith"));
    let hook = Command::new("sh")
        .arg(hooks.join("pre-commit"))
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", binary.parent().unwrap().display()),
        )
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(hook.status.code(), Some(0), "{hook:?}");
    let stderr = String::from_utf8_lossy(&hook.stderr);
    assert!(stderr.contains("foreign pre-commit ran"), "{stderr}");
    assert!(stderr.contains("26 files staged"), "{stderr}");

    // A foreign hook that refuses still stops the commit.
    std::fs::write(
        hooks.join("pre-commit.pre-commitsmith"),
        "#!/bin/sh\nexit 1\n",
    )
    .unwrap();
    make_executable(&hooks.join("pre-commit.pre-commitsmith"));
    let blocked = Command::new("sh")
        .arg(hooks.join("pre-commit"))
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", binary.parent().unwrap().display()),
        )
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        blocked.status.code(),
        Some(1),
        "its verdict stands: {blocked:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_pre_commit_hook_says_nothing_without_a_binary() {
    let dir = repo("pre-commit-silent");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let install = bin_with_home(&home)
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");
    run(&dir, "git", &["config", "--unset", "commitsmith.binary"]);

    let hook = Command::new("sh")
        .arg(dir.join(".git/hooks/pre-commit"))
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(hook.status.success(), "{hook:?}");
    assert!(hook.stdout.is_empty() && hook.stderr.is_empty(), "{hook:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A tiny HTTP server on a free port that serves files from `root`.
///
/// `commitsmith update` is the one command whose value is entirely in its
/// contract — verify before replacing, never leave the user without a binary —
/// and that cannot be tested without a release to download. A local server keeps
/// the test offline and lets it serve a deliberately broken checksum.
///
/// The thread stops when the returned guard is dropped, so a failed assertion
/// cannot leave a server waiting for a connection that never comes.
struct Server {
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    base: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.running.store(false, Ordering::SeqCst);
        // One cheap request so the blocking `read` returns and the loop sees the
        // flag.
        let _ = std::net::TcpStream::connect(self.base.trim_start_matches("http://"));
    }
}

fn serve(root: &Path) -> Server {
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let root = root.to_path_buf();
    let running = Arc::new(AtomicBool::new(true));
    let stop = Arc::clone(&running);
    std::thread::spawn(move || {
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            if !stop.load(Ordering::SeqCst) {
                break;
            }
            let mut buffer = [0u8; 2048];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_string();
            let file = root.join(path.trim_start_matches('/'));
            match std::fs::read(&file) {
                Ok(body) => {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(&body);
                }
                Err(_) => {
                    let _ =
                        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                }
            }
        }
    });
    Server { running, base }
}

/// A value that differs between two runs in the same process id.
fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}

fn sha256_of(path: &Path) -> String {
    let output = Command::new("sha256sum").arg(path).output().unwrap();
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// Lay out a fake release: an archive with a binary that reports `version`.
fn fake_release(root: &Path, version: &str, checksum: Option<&str>) -> String {
    let inner = format!("commitsmith-{version}-x86_64-unknown-linux-musl");
    let package = root.join("pkg").join(&inner);
    std::fs::create_dir_all(&package).unwrap();
    let script = format!("#!/bin/sh\necho 'commitsmith {version}'\n");
    let binary = package.join("commitsmith");
    std::fs::write(&binary, script).unwrap();
    make_executable(&binary);

    let archive_name = format!("{inner}.tar.gz");
    let status = Command::new("tar")
        .arg("czf")
        .arg(root.join(&archive_name))
        .arg("-C")
        .arg(root.join("pkg"))
        .arg(&inner)
        .status()
        .unwrap();
    assert!(status.success());

    let sum = checksum.map_or_else(|| sha256_of(&root.join(&archive_name)), str::to_string);
    std::fs::write(root.join("SHA256SUMS"), format!("{sum}  {archive_name}\n")).unwrap();
    archive_name
}

fn publish(root: &Path, version: &str, archive: &str) {
    let download = root.join("download").join(format!("v{version}"));
    std::fs::create_dir_all(&download).unwrap();
    std::fs::copy(root.join(archive), download.join(archive)).unwrap();
    std::fs::copy(root.join("SHA256SUMS"), download.join("SHA256SUMS")).unwrap();
    let listing = root.join("api/repos/mhinte/commitsmith/releases");
    std::fs::create_dir_all(&listing).unwrap();
    std::fs::write(
        listing.join("latest"),
        format!("{{\n  \"tag_name\": \"v{version}\",\n  \"name\": \"{version}\"\n}}\n"),
    )
    .unwrap();
}

#[test]
fn update_replaces_the_binary_only_after_verifying_it() {
    // A process id alone is not unique enough: a leftover helper from an earlier
    // run (same pid after reuse) copies into the same path and the next exec
    // fails with "Text file busy". The same trap the reword temp dir had.
    let root = std::env::temp_dir().join(format!(
        "commit-tui-update-{}-{}",
        std::process::id(),
        unique()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let archive = fake_release(&root, "9.9.9", None);
    publish(&root, "9.9.9", &archive);
    let server = serve(&root);
    let base = server.base.clone();

    // The binary to replace is a copy, never the running test binary.
    let install = root.join("install/commitsmith");
    std::fs::create_dir_all(install.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_commitsmith"), &install).unwrap();
    let env = [
        (
            "COMMITSMITH_API_BASE",
            format!("{base}/api/repos/mhinte/commitsmith"),
        ),
        ("COMMITSMITH_URL_BASE", format!("{base}/download")),
    ];
    // Starting a binary that a finishing helper is just replacing can fail with
    // "Text file busy" for a moment — that is Linux, not a bug (a person who runs
    // `update` and then `--version` in the same breath hits it too). The test
    // retries briefly instead of pretending the race is gone.
    let run_update = |args: &[&str]| {
        for attempt in 0..40u64 {
            let mut command = Command::new(&install);
            command.args(args).current_dir(&root);
            for (key, value) in &env {
                command.env(key, value);
            }
            match command.output() {
                Ok(output) => return output,
                Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
                }
                Err(error) => panic!("cannot run the binary under test: {error}"),
            }
        }
        panic!("the binary stayed busy for too long");
    };

    // `--check` answers without touching anything: exit 2 means "newer exists".
    let check = run_update(&["update", "--check"]);
    assert_eq!(check.status.code(), Some(2), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stdout).contains("9.9.9"));
    assert!(
        String::from_utf8_lossy(
            &Command::new(&install)
                .arg("--version")
                .output()
                .unwrap()
                .stdout
        )
        .contains(env!("CARGO_PKG_VERSION"))
    );

    let updated = run_update(&["update"]);
    assert!(updated.status.success(), "{updated:?}");
    // This process is executing the file, so the kernel refuses to write it
    // (ETXTBSY) and the update stages the new binary for a helper instead. That
    // is the normal Linux case, not a failure.
    assert!(
        String::from_utf8_lossy(&updated.stdout).contains("staged"),
        "{updated:?}"
    );
    assert!(
        root.join("install/commitsmith.staged").exists(),
        "the new binary waits for this process to end"
    );
    // Give the helper its moment: it copies as soon as the lock is gone (this
    // test process no longer executes the file), and removes the staged file.
    let staged = root.join("install/commitsmith.staged");
    // The helper polls once a second and a loaded CI runner is slow: give it a
    // generous window instead of pinning this test to this machine's speed.
    let finished = (0..120).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(250));
        !staged.exists()
    });
    assert!(finished, "the staged binary was finished by the helper");
    let mut version = String::new();
    for _ in 0..40 {
        match Command::new(&install).arg("--version").output() {
            Ok(output) => {
                version = String::from_utf8_lossy(&output.stdout).to_string();
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    assert!(
        version.contains("9.9.9"),
        "the installed binary is the new one: {version}"
    );
    // Either a copy of the old binary next to it, or the same file rewritten in
    // place (which is what a busy inode needs); never neither.
    assert!(
        root.join("install/commitsmith.old").exists()
            || !root.join("install/commitsmith.new").exists(),
        "no half-written file is left behind"
    );

    // And the release that was installed is now the newest one. The binary in
    // place is the fake, which does not know `update`, so the answer has to come
    // from a fresh copy of this build.
    std::fs::write(
        root.join("api/repos/mhinte/commitsmith/releases/latest"),
        format!("{{\"tag_name\": \"v{}\"}}\n", env!("CARGO_PKG_VERSION")),
    )
    .unwrap();
    let again = Command::new(env!("CARGO_BIN_EXE_commitsmith"))
        .args(["update"])
        .current_dir(&root)
        .env(env[0].0, &env[0].1)
        .env(env[1].0, &env[1].1)
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(0), "{again:?}");
    assert!(
        String::from_utf8_lossy(&again.stdout).contains("newest"),
        "{again:?}"
    );
    drop(server);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_wrong_checksum_leaves_the_binary_untouched() {
    let root = std::env::temp_dir().join(format!(
        "commit-tui-update-bad-{}-{}",
        std::process::id(),
        unique()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // A plausible-looking but wrong sum.
    let archive = fake_release(
        &root,
        "9.9.9",
        Some("0000000000000000000000000000000000000000000000000000000000000000"),
    );
    publish(&root, "9.9.9", &archive);
    let server = serve(&root);
    let base = server.base.clone();

    let install = root.join("install/commitsmith");
    std::fs::create_dir_all(install.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_commitsmith"), &install).unwrap();
    let before = std::fs::read(&install).unwrap();

    let output = Command::new(&install)
        .args(["update"])
        .current_dir(&root)
        .env(
            "COMMITSMITH_API_BASE",
            format!("{base}/api/repos/mhinte/commitsmith"),
        )
        .env("COMMITSMITH_URL_BASE", format!("{base}/download"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("checksum mismatch"),
        "{output:?}"
    );
    assert_eq!(
        std::fs::read(&install).unwrap(),
        before,
        "a failed verification must not touch the binary"
    );
    drop(server);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn hooks_without_an_action_needs_a_terminal_and_says_how_else() {
    // The wizard cannot run in a pipe: a script that calls `commitsmith hooks`
    // has to be told what to use instead, not left with a blank screen.
    let dir = repo("hooks-no-tty");
    let output = Command::new(env!("CARGO_BIN_EXE_commitsmith"))
        .arg("hooks")
        .current_dir(&dir)
        .stdout(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("commitsmith hooks install"), "{stderr}");
    assert!(stderr.contains("--yes"), "{stderr}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn hooks_yes_installs_all_three_without_asking() {
    let dir = repo("hooks-yes");
    let output = Command::new(env!("CARGO_BIN_EXE_commitsmith"))
        .args(["hooks", "--yes"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    for hook in ["commit-msg", "prepare-commit-msg", "pre-commit"] {
        assert!(dir.join(".git/hooks").join(hook).exists(), "{hook}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_post_checkout_hook_speaks_on_a_branch_switch_only() {
    let dir = repo("post-checkout-flag");
    let install = bin()
        .args(["hooks", "install"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(install.status.success(), "{install:?}");

    let binary = Path::new(env!("CARGO_BIN_EXE_commitsmith"));
    let path = format!("{}:/usr/bin:/bin", binary.parent().unwrap().display());
    let run_hook = |flag: &str| {
        Command::new("sh")
            .arg(dir.join(".git/hooks/post-checkout"))
            .args(["0000", "1111", flag])
            .env("PATH", &path)
            .current_dir(&dir)
            .output()
            .expect("run the hook")
    };

    // A real branch switch: git reports $3 = 1.
    run(
        &dir,
        "git",
        &["checkout", "-q", "-b", "feat/hima/TICKET-12/add-a-thing"],
    );
    let switched = run_hook("1");
    assert!(switched.status.success(), "{switched:?}");
    let stderr = String::from_utf8_lossy(&switched.stderr);
    assert!(
        stderr.contains("branch feat/hima/TICKET-12/add-a-thing"),
        "{stderr}"
    );
    assert!(stderr.contains("type feat"), "{stderr}");
    assert!(stderr.contains("TICKET-12"), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "one line: {stderr}");
    assert!(stderr.lines().all(|line| line.len() < 80), "{stderr}");

    // A file checkout: git reports $3 = 0 and the hook stays quiet.
    let files = run_hook("0");
    assert!(files.status.success(), "{files:?}");
    assert!(
        files.stdout.is_empty() && files.stderr.is_empty(),
        "a file checkout is not a branch switch: {files:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_branch_hint_says_nothing_about_a_branch_without_a_convention() {
    // `main` names no type and no ticket: no line, no noise on every switch.
    let dir = repo("branch-hint-quiet");
    std::fs::write(dir.join("a.txt"), "x").unwrap();
    run(&dir, "git", &["add", "a.txt"]);
    run(&dir, "git", &["commit", "-q", "-m", "feat: add a file"]);
    let output = bin()
        .args(["branch-hint"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "{output:?}"
    );

    // A detached HEAD has no branch to name either.
    run(&dir, "git", &["checkout", "-q", "--detach"]);
    let detached = bin()
        .args(["branch-hint"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(detached.status.success(), "{detached:?}");
    assert!(detached.stderr.is_empty(), "{detached:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_branch_hint_names_type_and_ticket_and_never_blocks() {
    let dir = repo("branch-hint-speaks");
    run(
        &dir,
        "git",
        &["checkout", "-q", "-b", "fix/hima/PROJ-7/typo"],
    );
    let output = bin()
        .args(["branch-hint"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.trim(),
        "commitsmith: branch fix/hima/PROJ-7/typo · type fix · PROJ-7",
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
