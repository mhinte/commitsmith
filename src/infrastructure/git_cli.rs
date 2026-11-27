//! The `git` command line, behind the ports.
//!
//! Writes go through `git` itself: a tool that reimplements `commit`, `merge` or
//! `push` eventually corrupts a repository. Reads reuse [`GixRepo`], so listing
//! does not spawn a process.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::errors::GitError;
use crate::domain::history::{PastCommit, ScopeInfo};
use crate::domain::ports::{BranchInfo, BranchOverview, ChangedFile, GitRepository};
use crate::infrastructure::gix_repo::{GixRepo, file_state};

/// A repository that reads with `gix` and writes with `git`.
pub struct GitBackend {
    reader: GixRepo,
    workdir: PathBuf,
    /// False in a repository without a commit. `reset --` cannot unstage there,
    /// `restore --staged` can, so the two answers are not interchangeable.
    has_head: bool,
}

impl GitBackend {
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let reader = GixRepo::open(path)?;
        let workdir = reader.workdir().to_path_buf();
        let has_head = Command::new("git")
            .current_dir(&workdir)
            .args(["rev-parse", "--verify", "HEAD"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        Ok(Self {
            reader,
            workdir,
            has_head,
        })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Run `git` and hand back stdout, or a [`GitError`] carrying stderr.
    fn git(&self, args: &[&str]) -> Result<String, GitError> {
        let output = Command::new("git")
            .current_dir(&self.workdir)
            .args(args)
            .output()
            .map_err(|error| GitError::io(format!("git {}", args.first().unwrap_or(&"")), error))?;
        if output.status.success() {
            // One copy: `from_utf8_lossy(..).to_string()` allocates twice, and a
            // diff of a large file goes through here.
            return Ok(match String::from_utf8(output.stdout) {
                Ok(text) => text,
                Err(error) => String::from_utf8_lossy(error.as_bytes()).to_string(),
            });
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(classify(args, &stderr, &self.workdir))
    }
}

/// Map git's answer onto the error the surface should show.
/// `%(upstream:track)` says `[ahead 2, behind 1]`, `[ahead 3]`, `[behind 4]` or
/// nothing at all. The numbers are all that matter here.
fn parse_track(track: &str) -> (usize, usize) {
    let body = track.trim_matches(|c| c == '[' || c == ']');
    let mut ahead = 0;
    let mut behind = 0;
    for part in body.split(',') {
        let mut words = part.split_whitespace();
        match (words.next(), words.next()) {
            (Some("ahead"), Some(count)) => ahead = count.parse().unwrap_or(0),
            (Some("behind"), Some(count)) => behind = count.parse().unwrap_or(0),
            _ => {}
        }
    }
    (ahead, behind)
}

/// Parse `git show --name-status` output into `M path` lines, at most twelve.
fn parse_name_status(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.replace('\t', " "))
        .take(12)
        .collect()
}

/// Parse `git status --porcelain=v1 -z` into changed files.
///
/// Records are NUL-terminated as `XY <path>`; a rename or copy is followed by
/// one extra record holding the path it came from. Nothing is quoted here, so a
/// space or an umlaut arrives as itself.
fn parse_status(output: &str) -> Vec<ChangedFile> {
    let mut changes: Vec<ChangedFile> = Vec::new();
    let mut records = output.split('\0');
    while let Some(record) = records.next() {
        let bytes = record.as_bytes();
        if bytes.len() < 4 {
            continue;
        }
        let index = bytes[0] as char;
        let worktree = bytes[1] as char;
        let path = &record[3..];
        // `??` is untracked: nothing in the index, and the file is new in the
        // worktree. Treating it as staged made `Space` unstage it, which failed
        // with "pathspec did not match".
        let state = |code: char| match code {
            ' ' | '?' => None,
            other => Some(file_state(other)),
        };
        let untracked = index == '?' && worktree == '?';
        let mut file = ChangedFile::new(
            path,
            state(index),
            if untracked {
                Some(crate::domain::ports::FileState::Untracked)
            } else {
                state(worktree)
            },
        );
        if matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C') {
            if let Some(origin) = records.next() {
                file = file.with_origin(origin);
            }
        }
        changes.push(file);
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

/// Parse one `for-each-ref` line per branch into the list and its sync marks.
fn parse_branch_overview(output: &str) -> BranchOverview {
    let mut branches: Vec<BranchInfo> = Vec::new();
    let mut sync: Vec<(String, usize, usize)> = Vec::new();
    for line in output.lines() {
        let mut fields = line.split('\0');
        let (Some(reference), Some(short), Some(head), upstream, track) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        // `refs/remotes/origin/HEAD` is a symbolic alias, not a branch.
        if short.ends_with("/HEAD") {
            continue;
        }
        let is_remote = reference.starts_with("refs/remotes/");
        branches.push(BranchInfo {
            name: short.to_string(),
            is_head: head.trim() == "*",
            is_remote,
            upstream: upstream
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        });
        // Only local branches carry a meaningful ahead/behind here, and `[gone]`
        // says the upstream was deleted: neither number exists then.
        if !is_remote {
            let track = track.unwrap_or_default().trim();
            if !track.is_empty() && track != "[gone]" {
                let (ahead, behind) = parse_track(track);
                sync.push((short.to_string(), ahead, behind));
            }
        }
    }
    BranchOverview { branches, sync }
}

fn classify(args: &[&str], stderr: &str, workdir: &Path) -> GitError {
    let operation = args.join(" ");
    let lower = stderr.to_lowercase();
    if lower.contains("not fully merged") || lower.contains("unmerged") {
        return GitError::conflict(Vec::<String>::new());
    }
    if lower.contains("no upstream") || lower.contains("no configured push destination") {
        return GitError::invalid_state(format!("no upstream for this branch: {stderr}"));
    }
    if lower.contains("would be overwritten") || lower.contains("local changes") {
        return GitError::invalid_state(format!("the working tree is in the way: {stderr}"));
    }
    if lower.contains("not a git repository") {
        return GitError::not_found(workdir.display().to_string());
    }
    GitError::remote(operation, stderr.to_string())
}

impl GitRepository for GitBackend {
    fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
        self.reader.current_branch()
    }

    /// Read the branches with `git`, not from the cached `gix` snapshot: a
    /// checkout rewrites `HEAD` behind that snapshot, which is why a freshly
    /// created branch used to stay grey until the program was restarted.
    fn branches(&self) -> Result<Vec<BranchInfo>, GitError> {
        Ok(self.branch_overview()?.branches)
    }

    /// One `for-each-ref` answers both questions: the branches and how far each
    /// one is from its upstream.
    fn branch_overview(&self) -> Result<BranchOverview, GitError> {
        let output = self.git(&[
            "for-each-ref",
            "--format=%(refname)%00%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(upstream:track)",
            "refs/heads",
            "refs/remotes",
        ])?;
        Ok(parse_branch_overview(&output))
    }

    fn changes(&self) -> Result<Vec<ChangedFile>, GitError> {
        // `git status` is the authority for untracked, ignored and conflict
        // states; reimplementing that with `gix` would only add a second truth.
        // `-z` is not cosmetic: without it git quotes every path that carries a
        // space or a non-ASCII byte (`"mit leer.txt"`, `"\303\274ber.txt"`) and
        // writes a move as `old.txt -> new.txt`, so `git add -- <path>` then
        // fails with "pathspec did not match any files".
        let output = self.git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        Ok(parse_status(&output))
    }

    fn recent_commits(&self, limit: usize) -> Result<Vec<PastCommit>, GitError> {
        self.reader.recent_commits(limit)
    }

    fn tags(&self) -> Result<Vec<String>, GitError> {
        self.reader.tags()
    }

    fn scopes(&self, configured: &[String], depth: usize) -> Result<ScopeInfo, GitError> {
        self.reader.scopes(configured, depth)
    }

    fn commits_since(
        &self,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::domain::history::PastCommit>, GitError> {
        self.reader.commits_since(since, limit)
    }

    fn commits_not_on(&self, base: &str, limit: usize) -> Result<Vec<PastCommit>, GitError> {
        // The merge base is the honest cut: everything after it is this branch's
        // own work. Walking far enough matters — the base can be deep.
        let merge_base = self.git(&["merge-base", base, "HEAD"])?;
        let sha = merge_base.trim();
        if sha.is_empty() {
            return Err(GitError::invalid_state(format!(
                "no common history with {base}"
            )));
        }
        // The merge base goes into the walk as a hidden tip, so a deep base no
        // longer needs a 500-commit head start to be sure it is inside the
        // window.
        self.reader.commits_before(sha, limit)
    }

    /// `git log -1 --format=%B` — the message as it was recorded, footers and all,
    /// so amending it cannot silently drop a `BREAKING CHANGE:` line.
    fn head_message(&self) -> Result<Option<String>, GitError> {
        match self.git(&["log", "-1", "--format=%B"]) {
            Ok(text) if !text.trim().is_empty() => Ok(Some(text)),
            // A repository without a commit has nothing to amend.
            Ok(_) | Err(GitError::InvalidState { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Replace the message of the last commit. The tree is untouched, so this is
    /// only about words — `--only` would silently drop staged files.
    fn amend(&self, message: &str) -> Result<String, GitError> {
        let output = self.git(&["commit", "--amend", "-m", message])?;
        short_id(&output).ok_or_else(|| GitError::invalid_state("git printed no commit id"))
    }

    /// `git show --name-status` for one commit. Through the shared `git` helper,
    /// so a failure is classified and reported instead of silently yielding an
    /// empty list.
    fn commit_files(&self, id: &str) -> Result<Vec<String>, GitError> {
        let output = self.git(&[
            "show",
            "--no-commit-id",
            "--name-status",
            "--format=",
            "-M",
            id,
        ])?;
        Ok(parse_name_status(&output))
    }

    fn diff(&self, path: &str, staged: bool) -> Result<String, GitError> {
        // No pager, no colour: the pane decides how to paint the hunks.
        let args: Vec<&str> = if staged {
            vec![
                "diff",
                "--cached",
                "--no-color",
                "--no-ext-diff",
                "--",
                path,
            ]
        } else {
            vec!["diff", "--no-color", "--no-ext-diff", "--", path]
        };
        self.git(&args)
    }

    fn stage(&self, path: &str) -> Result<(), GitError> {
        self.git(&["add", "--", path]).map(|_| ())
    }

    /// `-A` with both sides: the new path goes in, the old one is recorded as
    /// deleted. With the new path alone the move never reaches the index.
    fn stage_move(&self, origin: &str, path: &str) -> Result<(), GitError> {
        self.git(&["add", "-A", "--", origin, path]).map(|_| ())
    }

    fn unstage(&self, path: &str) -> Result<(), GitError> {
        // `reset --` puts the index back to HEAD and also handles a *staged
        // deletion*, which `restore --staged` refuses ("pathspec did not
        // match"). A repository without HEAD has nothing to reset to, so
        // `restore --staged` stays the answer there.
        let args: Vec<&str> = if self.has_head {
            vec!["reset", "-q", "--", path]
        } else {
            vec!["restore", "--staged", "--", path]
        };
        self.git(&args).map(|_| ())
    }

    /// Both sides, or the old path keeps sitting in the index as a deletion.
    fn unstage_move(&self, origin: &str, path: &str) -> Result<(), GitError> {
        let args: Vec<&str> = if self.has_head {
            vec!["reset", "-q", "--", origin, path]
        } else {
            vec!["restore", "--staged", "--", origin, path]
        };
        self.git(&args).map(|_| ())
    }

    fn discard(&self, path: &str) -> Result<(), GitError> {
        // `restore` without `--staged` rewrites the file from the index; that is
        // what "discard my changes" means and it cannot be undone.
        self.git(&["restore", "--", path]).map(|_| ())
    }

    fn stash(&self, message: &str) -> Result<(), GitError> {
        self.git(&["stash", "push", "-u", "-m", message])
            .map(|_| ())
    }

    fn stash_pop(&self) -> Result<(), GitError> {
        self.git(&["stash", "pop"]).map(|_| ())
    }

    fn commit(&self, message: &str) -> Result<String, GitError> {
        let output = self.git(&["commit", "-m", message])?;
        short_id(&output).ok_or_else(|| GitError::invalid_state("git printed no commit id"))
    }

    fn checkout(&self, branch: &str) -> Result<(), GitError> {
        self.git(&["checkout", branch]).map(|_| ())
    }

    fn create_branch(&self, name: &str) -> Result<(), GitError> {
        self.git(&["checkout", "-b", name]).map(|_| ())
    }

    fn merge(&self, branch: &str) -> Result<(), GitError> {
        self.git(&["merge", "--no-edit", branch]).map(|_| ())
    }

    fn delete_branch(&self, branch: &str) -> Result<(), GitError> {
        self.git(&["branch", "-d", branch]).map(|_| ())
    }

    fn pull(&self) -> Result<(), GitError> {
        self.git(&["pull", "--ff-only"]).map(|_| ())
    }

    fn push(&self) -> Result<(), GitError> {
        self.git(&["push"]).map(|_| ())
    }

    fn branch_sync(&self) -> Result<Vec<(String, usize, usize)>, GitError> {
        Ok(self.branch_overview()?.sync)
    }

    fn fetch(&self) -> Result<(), GitError> {
        self.git(&["fetch", "--prune", "--quiet"]).map(|_| ())
    }

    fn create_tag(&self, tag: &str, message: &str) -> Result<(), GitError> {
        self.git(&["tag", "-a", tag, "-m", message]).map(|_| ())
    }

    /// `git rev-list --left-right --count HEAD...@{upstream}` answers both numbers
    /// in one call: left is ahead, right is behind. A missing upstream is not an
    /// error — it is a state the caller has to explain to the user — and git
    /// says so itself ("no upstream configured"), so there is no probing call
    /// in front of this one.
    fn ahead_behind(&self) -> Result<Option<(usize, usize)>, GitError> {
        let output = match self.git(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        {
            Ok(output) => output,
            Err(GitError::InvalidState { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut parts = output.split_whitespace();
        let ahead: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let behind: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        Ok(Some((ahead, behind)))
    }
}

/// Point a git alias at this binary: `git smith` then opens the composer.
///
/// `!` makes git run the command through a shell. That is why the path needs
/// forward slashes and quotes: a shell reads `C:\Users\marc` as `C:Usersmarc`,
/// which is exactly the broken alias a Windows user reported.
pub fn install_alias(name: &str, binary: &str) -> Result<(), GitError> {
    install_alias_in(name, binary, None)
}

/// Same, but with an explicit global config file (used by tests).
pub fn install_alias_in(
    name: &str,
    binary: &str,
    global_config: Option<&Path>,
) -> Result<(), GitError> {
    let key = format!("alias.{name}");
    let value = shell_command(binary);
    let mut command = Command::new("git");
    command.args(["config", "--global", &key, &value]);
    if let Some(path) = global_config {
        command.env("GIT_CONFIG_GLOBAL", path);
    }
    let output = command
        .output()
        .map_err(|error| GitError::io("git config --global", error))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::remote(
            "git config",
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// Quote a binary path for git's `!` aliases.
///
/// Windows paths become `!"C:/Program Files/commitsmith.exe"`: forward slashes
/// because the shell strips backslashes, quotes because the path may contain
/// spaces.
fn shell_command(binary: &str) -> String {
    let path = binary.replace('\\', "/");
    if path.contains(' ') || path.contains('"') {
        format!("!\"{}\"", path.replace('"', "\\\""))
    } else {
        format!("!{path}")
    }
}

/// The `[main abc1234] subject` line, reduced to the id.
///
/// A root commit prints `[main (root-commit) abc1234]`, so the id is the last
/// token inside the brackets, not the second one.
fn short_id(output: &str) -> Option<String> {
    let line = output.lines().next()?;
    let inner = line.split('[').nth(1)?.split(']').next()?;
    let id = inner.split_whitespace().last()?;
    Some(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::FileState;
    #[test]
    fn a_branch_created_outside_the_program_is_head_at_once() {
        let (dir, repo) = temp_repo("fresh-head");
        repo.stage("a.txt").ok();
        repo.commit("feat: first").unwrap();
        assert!(
            Command::new("git")
                .args(["checkout", "-q", "-b", "feature/fresh"])
                .current_dir(&dir)
                .status()
                .unwrap()
                .success()
        );
        let branches = repo.branches().unwrap();
        let head = branches.iter().find(|b| b.is_head).expect("one head");
        assert_eq!(head.name, "feature/fresh");
        assert_eq!(branches.iter().filter(|b| b.is_head).count(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn remote_branches_are_marked_and_have_their_upstream() {
        let (dir, repo) = temp_repo("remote-list");
        repo.stage("a.txt").ok();
        repo.commit("feat: first").unwrap();
        let bare = dir.join("remote.git");
        assert!(
            Command::new("git")
                .args(["init", "-q", "--bare"])
                .arg(&bare)
                .status()
                .unwrap()
                .success()
        );
        repo.git(&["remote", "add", "origin", bare.to_str().unwrap()])
            .unwrap();
        repo.git(&["push", "-q", "-u", "origin", "main"]).unwrap();
        repo.git(&["fetch", "-q", "origin"]).unwrap();

        let branches = repo.branches().unwrap();
        let local = branches.iter().find(|b| b.name == "main").unwrap();
        assert!(!local.is_remote);
        assert_eq!(local.upstream.as_deref(), Some("origin/main"));
        assert!(
            branches
                .iter()
                .any(|b| b.name == "origin/main" && b.is_remote)
        );
        let _ = fs::remove_dir_all(dir);
    }

    use std::fs;

    fn temp_repo(name: &str) -> (PathBuf, GitBackend) {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-cli-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            let status = Command::new("git")
                .current_dir(&dir)
                .args(&args)
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        }
        fs::write(dir.join("a.txt"), "hello\n").unwrap();
        let backend = GitBackend::open(&dir).unwrap();
        (dir, backend)
    }

    #[test]
    fn one_for_each_ref_line_answers_branches_and_sync() {
        // `%(HEAD)` is `*` on the current branch; `%(upstream:track)` is empty
        // without an upstream and `[gone]` when it was deleted.
        let output = "refs/heads/main\0main\0*\0origin/main\0[ahead 2, behind 1]\n\
                      refs/heads/local\0local\0 \0\0\n\
                      refs/heads/stale\0stale\0 \0origin/stale\0[gone]\n\
                      refs/remotes/origin/main\0origin/main\0 \0\0\n\
                      refs/remotes/origin/HEAD\0origin/HEAD\0 \0\0\n";
        let overview = parse_branch_overview(output);
        let names: Vec<&str> = overview
            .branches
            .iter()
            .map(|branch| branch.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["main", "local", "stale", "origin/main"],
            "{names:?}"
        );
        assert!(overview.branches[0].is_head);
        assert_eq!(
            overview.branches[0].upstream.as_deref(),
            Some("origin/main")
        );
        assert!(
            overview.branches.iter().all(|b| b.name != "origin/HEAD"),
            "the symbolic alias is not a branch"
        );
        assert_eq!(
            overview.sync,
            vec![("main".to_string(), 2, 1)],
            "only a live upstream on a local branch has numbers"
        );
    }

    #[test]
    fn the_status_parser_reads_moves_and_unquoted_paths() {
        // Exactly what `git status --porcelain=v1 -z` prints: the rename record
        // is followed by the path it came from, and nothing is quoted.
        let output = "M  mit leer.txt\u{0}R  new.txt\u{0}old.txt\u{0} M \u{fc}ber.txt\u{0}?? frisch.txt\u{0}";
        let files = parse_status(output);
        assert_eq!(files.len(), 4, "{files:?}");
        let space = files
            .iter()
            .find(|f| f.path == "mit leer.txt")
            .expect("space");
        assert_eq!(space.index, Some(FileState::Modified));
        let moved = files.iter().find(|f| f.path == "new.txt").expect("new");
        assert_eq!(moved.origin.as_deref(), Some("old.txt"));
        assert_eq!(moved.index, Some(FileState::Renamed));
        assert_eq!(moved.paths(), ["old.txt", "new.txt"]);
        let umlaut = files
            .iter()
            .find(|f| f.path == "\u{fc}ber.txt")
            .expect("umlaut");
        assert_eq!(umlaut.worktree, Some(FileState::Modified));
        let fresh = files
            .iter()
            .find(|f| f.path == "frisch.txt")
            .expect("new file");
        assert_eq!(fresh.worktree, Some(FileState::Untracked));
        assert!(fresh.origin.is_none());
    }

    #[test]
    fn paths_with_a_space_or_an_umlaut_can_be_staged() {
        let (dir, repo) = temp_repo("special-paths");
        // `restore --staged` needs a commit to restore against.
        repo.stage("a.txt").unwrap();
        repo.commit("chore: init").unwrap();
        std::fs::write(dir.join("mit leer.txt"), "x\n").unwrap();
        std::fs::write(dir.join("\u{fc}ber.txt"), "y\n").unwrap();
        for name in ["mit leer.txt", "\u{fc}ber.txt"] {
            let file = repo
                .changes()
                .unwrap()
                .into_iter()
                .find(|file| file.path == name)
                .unwrap_or_else(|| panic!("{name} missing in status"));
            repo.stage(&file.path)
                .unwrap_or_else(|error| panic!("stage {name}: {error}"));
            let staged = repo
                .changes()
                .unwrap()
                .into_iter()
                .find(|file| file.path == name)
                .expect("still listed");
            assert!(staged.is_staged(), "{name} not in the index");
            // The diff of the quoted path used to come back empty.
            repo.diff(&file.path, true)
                .unwrap_or_else(|error| panic!("diff {name}: {error}"));
            repo.unstage(&file.path)
                .unwrap_or_else(|error| panic!("unstage {name}: {error}"));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_move_is_staged_with_both_of_its_paths() {
        let (dir, repo) = temp_repo("move");
        repo.stage("a.txt").unwrap();
        repo.commit("chore: init").unwrap();
        let status = Command::new("git")
            .current_dir(&dir)
            .args(["mv", "a.txt", "b.txt"])
            .status()
            .unwrap();
        assert!(status.success());

        // `git mv` stages the move, so status names both ends.
        let staged = repo
            .changes()
            .unwrap()
            .into_iter()
            .find(|file| file.path == "b.txt")
            .expect("b.txt listed");
        assert_eq!(staged.origin.as_deref(), Some("a.txt"));
        assert_eq!(staged.index, Some(FileState::Renamed));
        assert_eq!(staged.label(), "a.txt \u{2192} b.txt");

        // Both sides out of the index: one alone leaves the old path behind as
        // a *staged* deletion, so nothing would be free.
        repo.unstage_move("a.txt", "b.txt").unwrap();
        let after = repo.changes().unwrap();
        assert!(
            !after
                .iter()
                .any(crate::domain::ports::ChangedFile::is_staged),
            "nothing may stay staged: {after:?}"
        );
        assert!(
            after
                .iter()
                .any(|file| file.path == "b.txt" && file.worktree == Some(FileState::Untracked)),
            "{after:?}"
        );
        // A staged deletion cannot be freed with `restore --staged`; unstage on
        // the old path alone has to work as well.
        repo.stage("a.txt").unwrap();
        repo.unstage("a.txt").unwrap();
        assert!(
            !repo
                .changes()
                .unwrap()
                .iter()
                .any(crate::domain::ports::ChangedFile::is_staged)
        );

        // Both sides together bring the rename back into the index.
        repo.stage_move("a.txt", "b.txt").unwrap();
        let staged = repo
            .changes()
            .unwrap()
            .into_iter()
            .find(|file| file.path == "b.txt")
            .expect("b.txt listed");
        assert_eq!(staged.origin.as_deref(), Some("a.txt"));
        assert!(
            !repo
                .changes()
                .unwrap()
                .iter()
                .any(|file| file.path == "a.txt"),
            "the old path must not stay behind as an unstaged deletion"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_untracked_file_is_not_staged() {
        let (dir, repo) = temp_repo("untracked");
        std::fs::write(dir.join("new.txt"), "x").unwrap();
        let changes = repo.changes().unwrap();
        let file = changes
            .iter()
            .find(|file| file.path == "new.txt")
            .expect("new.txt");
        assert!(!file.is_staged(), "?? must not count as staged");
        assert_eq!(
            file.worktree,
            Some(crate::domain::ports::FileState::Untracked)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stage_commit_and_read_back() {
        let (dir, repo) = temp_repo("stage");
        repo.stage("a.txt").unwrap();
        assert!(
            repo.changes()
                .unwrap()
                .iter()
                .any(|c| c.is_staged() && c.path == "a.txt")
        );
        let id = repo.commit("feat: add a file").unwrap();
        assert_eq!(id.len(), 7, "short id: {id}");
        let commits = repo.recent_commits(5).unwrap();
        assert_eq!(commits[0].summary, "add a file");
        assert_eq!(repo.current_branch().unwrap().unwrap().name, "main");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn branches_can_be_created_and_deleted() {
        let (dir, repo) = temp_repo("branches");
        repo.stage("a.txt").unwrap();
        repo.commit("feat: base").unwrap();
        repo.create_branch("feat/mmu/T-1/x").unwrap();
        assert_eq!(
            repo.current_branch().unwrap().unwrap().name,
            "feat/mmu/T-1/x"
        );
        repo.checkout("main").unwrap();
        assert!(repo.delete_branch("feat/mmu/T-1/x").is_ok());
        let names: Vec<String> = repo
            .branches()
            .unwrap()
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec!["main"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn push_and_pull_against_a_local_remote() {
        let (dir, repo) = temp_repo("remote");
        let remote = dir.join("remote.git");
        let status = Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(&remote)
            .status()
            .unwrap();
        assert!(status.success());
        repo.stage("a.txt").unwrap();
        repo.commit("feat: first").unwrap();
        let remote_str = remote.to_str().unwrap().to_string();
        repo.git(&["remote", "add", "origin", &remote_str]).unwrap();

        // Without an upstream git refuses; that has to be a readable error.
        let error = repo.push().unwrap_err().to_string();
        assert!(error.contains("push"), "{error}");

        repo.git(&["push", "-q", "-u", "origin", "main"]).unwrap();
        assert!(repo.pull().is_ok(), "pull after setting the upstream");

        // A second clone sees the commit the push created.
        let clone_dir = dir.join("clone");
        // `-b main` on purpose: a bare repository's HEAD points at whatever
        // `init.defaultBranch` was, and on a CI runner that is still `master`,
        // so a plain clone would check out an unborn branch and see no log.
        let status = Command::new("git")
            .args(["clone", "-q", "-b", "main"])
            .arg(&remote)
            .arg(&clone_dir)
            .status()
            .unwrap();
        assert!(status.success());
        let log = Command::new("git")
            .args(["log", "--format=%s"])
            .current_dir(&clone_dir)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "feat: first");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_track_string_becomes_two_numbers() {
        assert_eq!(parse_track("[ahead 2, behind 1]"), (2, 1));
        assert_eq!(parse_track("[ahead 3]"), (3, 0));
        assert_eq!(parse_track("[behind 4]"), (0, 4));
        assert_eq!(parse_track(""), (0, 0));
        assert_eq!(parse_track("[gone]"), (0, 0));
    }

    #[test]
    fn ahead_behind_counts_what_is_missing_on_either_side() {
        let (dir, repo) = temp_repo("ahead-behind");
        let remote = dir.join("remote.git");
        assert!(
            Command::new("git")
                .args(["init", "-q", "--bare"])
                .arg(&remote)
                .status()
                .unwrap()
                .success()
        );
        repo.stage("a.txt").unwrap();
        repo.commit("feat: first").unwrap();
        let remote_str = remote.to_str().unwrap().to_string();
        repo.git(&["remote", "add", "origin", &remote_str]).unwrap();

        // No upstream yet: a state, not an error.
        assert_eq!(repo.ahead_behind().unwrap(), None);

        repo.git(&["push", "-q", "-u", "origin", "main"]).unwrap();
        assert_eq!(repo.ahead_behind().unwrap(), Some((0, 0)));

        // One local commit: ahead by one.
        fs::write(dir.join("a.txt"), "second\n").unwrap();
        repo.stage("a.txt").unwrap();
        repo.commit("feat: second").unwrap();
        assert_eq!(repo.ahead_behind().unwrap(), Some((1, 0)));

        // A commit from elsewhere, fetched: behind by one.
        let clone = dir.join("clone");
        assert!(
            Command::new("git")
                .args(["clone", "-q", "-b", "main"])
                .arg(&remote)
                .arg(&clone)
                .status()
                .unwrap()
                .success()
        );
        for args in [
            vec!["config", "user.email", "other@example.com"],
            vec!["config", "user.name", "Other"],
        ] {
            Command::new("git")
                .args(&args)
                .current_dir(&clone)
                .status()
                .unwrap();
        }
        fs::write(clone.join("b.txt"), "remote\n").unwrap();
        Command::new("git")
            .args(["add", "b.txt"])
            .current_dir(&clone)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "feat: remote"])
            .current_dir(&clone)
            .status()
            .unwrap();
        Command::new("git")
            .args(["push", "-q", "origin", "main"])
            .current_dir(&clone)
            .status()
            .unwrap();
        repo.git(&["fetch", "-q", "origin"]).unwrap();

        let (ahead, behind) = repo.ahead_behind().unwrap().unwrap();
        assert_eq!((ahead, behind), (1, 1), "diverged: one each way");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_command_becomes_a_error_not_a_panic() {
        let (dir, repo) = temp_repo("failing");
        let error = repo.checkout("does-not-exist").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("does-not-exist"), "{message}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn windows_paths_are_quoted_with_forward_slashes() {
        // git runs `!` aliases through a shell, which eats the backslashes and
        // ends up with `C:Usersmarc...` — the bug from issue #1.
        assert_eq!(
            shell_command(r"C:\Users\marc.hinte\commitsmith.exe"),
            "!C:/Users/marc.hinte/commitsmith.exe"
        );
        assert_eq!(
            shell_command(r"C:\Program Files\commitsmith.exe"),
            "!\"C:/Program Files/commitsmith.exe\""
        );
        assert_eq!(
            shell_command("/usr/local/bin/commitsmith"),
            "!/usr/local/bin/commitsmith"
        );
    }

    #[test]
    fn the_alias_is_stored_in_the_form_a_shell_can_run() {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-alias-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("gitconfig");
        fs::write(&config, "").unwrap();

        install_alias_in("smith", r"C:\Users\marc\commitsmith.exe", Some(&config)).unwrap();
        let stored = fs::read_to_string(&config).unwrap();
        assert!(stored.contains("[alias]"), "{stored}");
        assert!(
            stored.contains("smith = !C:/Users/marc/commitsmith.exe"),
            "{stored}"
        );
        assert!(!stored.contains(r"\Users"), "backslashes survive: {stored}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_short_id_parser_survives_other_output() {
        assert_eq!(
            short_id("[main 1a2b3c4] feat: x\n 1 file changed").as_deref(),
            Some("1a2b3c4")
        );
        assert_eq!(
            short_id("[main (root-commit) abc1234] feat: first").as_deref(),
            Some("abc1234")
        );
        assert_eq!(short_id("nothing here"), None);
    }
}
