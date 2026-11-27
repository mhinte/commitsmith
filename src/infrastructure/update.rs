//! Fetching a release: the only part of `commitsmith update` that touches the
//! network or the file system.
//!
//! Everything here talks to the endpoints the install script already uses, so a
//! corporate proxy that allows the curl installer allows this too. Nothing is
//! written until the checksum of the downloaded archive matches the published
//! one — a failed verification has to leave the running binary untouched.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::release::SemVer;
use crate::domain::update::{Decision, Platform, checksum_for, decide};

const REPO: &str = "mhinte/commitsmith";

/// The release endpoint, overridable so tests never touch GitHub.
fn api_base() -> String {
    std::env::var("COMMITSMITH_API_BASE")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}"))
}

/// What the release listing said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub tag: String,
}

/// Ask GitHub for the newest release tag.
///
/// The JSON is not parsed with a library: one string field out of a known
/// response does not justify a parser dependency, and `serde_json` is not in this
/// tree. The same `sed` approach the install script uses.
pub fn latest_tag() -> Result<Latest, String> {
    let url = format!("{}/releases/latest", api_base());
    let body = curl(&url)?;
    let tag = body
        .lines()
        .find_map(|line| {
            let rest = line.split("\"tag_name\"").nth(1)?;
            let rest = rest.split('"').nth(1)?;
            (!rest.is_empty()).then(|| rest.to_string())
        })
        .ok_or_else(|| format!("no tag_name in the answer from {url}"))?;
    Ok(Latest { tag })
}

fn curl(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "-H", "Accept: application/vnd.github+json", url])
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(match stderr.is_empty() {
        true => format!("cannot read {url}"),
        false => format!("cannot read {url}: {stderr}"),
    })
}

/// Download an asset into `dir` and check it against `SHA256SUMS`.
pub fn download(version: &str, asset: &str, dir: &Path) -> Result<PathBuf, String> {
    let base = std::env::var("COMMITSMITH_URL_BASE")
        .unwrap_or_else(|_| format!("https://github.com/{REPO}/releases/download"));
    let sums = curl(&format!("{base}/v{version}/SHA256SUMS")).map_err(|error| {
        format!(
            "{error}\nthe release publishes no checksums, refusing to replace a binary unverified"
        )
    })?;
    let expected = checksum_for(&sums, asset)
        .ok_or_else(|| format!("SHA256SUMS has no line for {asset}"))?
        .to_string();

    let path = dir.join(asset);
    let output = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&path)
        .arg(format!("{base}/v{version}/{asset}"))
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot download {asset}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let actual = sha256(&path)?;
    if actual != expected {
        return Err(format!(
            "checksum mismatch for {asset}\nexpected {expected}\ngot      {actual}"
        ));
    }
    Ok(path)
}

/// `sha256sum` / `shasum -a 256`, whichever this machine has.
fn sha256(path: &Path) -> Result<String, String> {
    for (program, args) in [("sha256sum", vec![]), ("shasum", vec!["-a", "256"])] {
        let Ok(output) = Command::new(program).args(&args).arg(path).output() else {
            continue;
        };
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            if let Some(sum) = text.split_whitespace().next() {
                return Ok(sum.to_lowercase());
            }
        }
    }
    Err("no sha256sum or shasum on this machine".to_string())
}

/// Unpack the archive and hand back the binary inside it.
pub fn unpack(
    archive: &Path,
    platform: Platform,
    version: &str,
    dir: &Path,
) -> Result<PathBuf, String> {
    let name = archive
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let status = match name.ends_with(".zip") {
        true => Command::new("unzip")
            .args(["-q", "-o"])
            .arg(archive)
            .arg("-d")
            .arg(dir)
            .status(),
        false => Command::new("tar")
            .args(["-xzf"])
            .arg(archive)
            .arg("-C")
            .arg(dir)
            .status(),
    }
    .map_err(|error| format!("cannot unpack {name}: {error}"))?;
    if !status.success() {
        return Err(format!("cannot unpack {name}"));
    }
    let relative = platform
        .binary_in_archive(version)
        .ok_or_else(|| format!("no binary path known for {platform:?}"))?;
    let binary = dir.join(&relative);
    if !binary.is_file() {
        return Err(format!("the archive did not contain {relative}"));
    }
    Ok(binary)
}

/// Replace `target` with `new`.
///
/// The running binary cannot always be overwritten or renamed on Linux
/// (`ETXTBSY`: the kernel refuses to write a file it is executing), so the update
/// has two stages. The new binary is copied next to the target as
/// `<name>.staged`, its `--version` is checked, and then:
///
/// - normally the bytes are written over the target, which keeps the same inode
///   (a hard link or a shell's path cache still points at the right file);
/// - if the file is busy, a small detached shell waits for the lock to go and
///   does the same copy. The command returns immediately and says so.
///
/// Nothing half-written is ever left: a failed check removes the staged file and
/// the old binary stays exactly as it was.
pub fn replace(target: &Path, new: &Path) -> Result<(), String> {
    let staged = sibling(target, "staged");
    std::fs::copy(new, &staged)
        .map_err(|error| format!("cannot write {}: {error}", staged.display()))?;

    let check = Command::new(&staged).arg("--version").output();
    match check {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let _ = std::fs::remove_file(&staged);
            return Err(format!(
                "the downloaded binary does not run: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Err(error) => {
            let _ = std::fs::remove_file(&staged);
            return Err(format!(
                "cannot run the downloaded binary: {error}\nthe old one is unchanged"
            ));
        }
    }

    match std::fs::copy(&staged, target) {
        Ok(_) => {
            let _ = std::fs::remove_file(&staged);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
            // Stage the finishing step and leave: this process is the one holding
            // the lock, so it cannot wait for itself to exit.
            spawn_finisher(&staged, target)?;
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&staged);
            Err(format!(
                "cannot write {}: {error}\nthe old one is unchanged",
                target.display()
            ))
        }
    }
}

/// True when an earlier update left work for a helper: the new binary is staged
/// and waiting for the running process to end.
pub fn is_pending(target: &Path) -> bool {
    sibling(target, "staged").exists()
}

/// `path` with its suffix replaced by `suffix`, in the same directory.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "commitsmith".to_string());
    let stem = name.split('.').next().unwrap_or(&name).to_string();
    path.with_file_name(format!("{stem}.{suffix}"))
}

/// Start the detached helper that finishes a busy update.
fn spawn_finisher(staged: &Path, target: &Path) -> Result<(), String> {
    // Only `sh` and `cp` are used: both are always there, and the loop is bounded
    // so a stuck lock leaves the staged file in place instead of a stray process
    // forever.
    let script = format!(
        "i=0\n\
         while [ $i -lt 60 ]; do\n\
         \tif cp \"{staged}\" \"{target}\" 2>/dev/null; then\n\
         \t\trm -f \"{staged}\"\n\
         \t\texit 0\n\
         \tfi\n\
         \ti=$((i+1))\n\
         \tsleep 1\n\
         done\n\
         exit 1\n",
        staged = staged.display(),
        target = target.display()
    );
    Command::new("sh")
        .args(["-c", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("cannot start the finishing step: {error}"))
}

/// The version the given binary reports, if it can be run.
pub fn version_of(binary: &Path) -> Option<SemVer> {
    let output = Command::new(binary).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.split_whitespace().last()?;
    SemVer::parse(version)
}

/// Everything `commitsmith update` does, in one place so `main` stays thin.
pub enum Outcome {
    UpToDate(String),
    Updated { from: String, to: String },
}

pub fn run(check_only: bool) -> Result<Outcome, String> {
    let platform =
        Platform::current().ok_or_else(|| format!("no release for {}", std::env::consts::OS))?;
    let ours = crate::domain::update::current();
    let latest = latest_tag()?;
    match decide(ours, &latest.tag) {
        Decision::Unreadable(tag) => {
            Err(format!("`{tag}` is not a version this build understands"))
        }
        Decision::UpToDate { current } => Ok(Outcome::UpToDate(current)),
        Decision::Newer { latest } => {
            if check_only {
                return Ok(Outcome::Updated {
                    from: ours
                        .map(|version| {
                            format!("{}.{}.{}", version.major, version.minor, version.patch)
                        })
                        .unwrap_or_else(|| "?".to_string()),
                    to: latest,
                });
            }
            let target = std::env::current_exe()
                .map_err(|error| format!("cannot tell where this binary lives: {error}"))?;
            let dir =
                std::env::temp_dir().join(format!("commitsmith-update-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir)
                .map_err(|error| format!("cannot use {}: {error}", dir.display()))?;

            let asset = platform.asset_name(&latest);
            let result = download(&latest, &asset, &dir)
                .and_then(|archive| unpack(&archive, platform, &latest, &dir))
                .and_then(|binary| replace(&target, &binary));
            let _ = std::fs::remove_dir_all(&dir);
            result?;
            Ok(Outcome::Updated {
                from: ours
                    .map(|version| format!("{}.{}.{}", version.major, version.minor, version.patch))
                    .unwrap_or_else(|| "?".to_string()),
                to: latest,
            })
        }
    }
}
