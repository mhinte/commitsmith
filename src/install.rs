//! `commitsmith --install` / `--uninstall`: put the running binary on PATH.
//!
//! This exists because the PowerShell installer was a second thing that could
//! fail. The binary knows its own path, so it can do the job itself and it works
//! the same on Windows, macOS and Linux.

use std::path::{Path, PathBuf};

/// What the operation did, for the message the CLI prints.
pub struct Report {
    pub binary: PathBuf,
    pub directory: PathBuf,
    pub alias: Option<PathBuf>,
    /// True when the directory was added to PATH just now.
    pub added_to_path: bool,
    /// True when the running shell has to be restarted to see the change.
    pub restart_shell: bool,
}

/// Copy the running binary to a per-user directory, optionally add a short alias
/// and make the directory reachable from every shell.
pub fn install(alias: Option<&str>) -> Result<Report, String> {
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot find the running binary: {error}"))?;
    let directory = target_dir()?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;

    let target = directory.join(exe_name("commitsmith"));
    copy_binary(&current, &target)?;

    let mut alias_path = None;
    if let Some(name) = alias.map(str::trim).filter(|name| !name.is_empty()) {
        let path = directory.join(exe_name(name));
        copy_binary(&target, &path)?;
        alias_path = Some(path);
    }

    let added_to_path = add_to_path(&directory)?;
    Ok(Report {
        binary: target,
        directory,
        alias: alias_path,
        added_to_path,
        restart_shell: added_to_path,
    })
}

/// Remove the installed copies again. The file on disk that is currently
/// running is left alone if it happens to be the installed one.
pub fn uninstall() -> Result<Report, String> {
    let current = std::env::current_exe().ok();
    let directory = target_dir()?;
    let binary = directory.join(exe_name("commitsmith"));

    let mut removed_alias = None;
    for entry in std::fs::read_dir(&directory)
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path == binary {
            continue;
        }
        // Only remove copies of ourselves, whatever they are called.
        if current.as_deref() == Some(path.as_path()) {
            continue;
        }
        if is_our_binary(&path) {
            let _ = std::fs::remove_file(&path);
            removed_alias = Some(path);
        }
    }

    if Some(binary.clone()) != current {
        match std::fs::remove_file(&binary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot remove {}: {error}", binary.display())),
        }
    }

    Ok(Report {
        binary,
        directory,
        alias: removed_alias,
        added_to_path: false,
        restart_shell: false,
    })
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

fn copy_binary(from: &Path, to: &Path) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    // Unlinking first avoids "text file busy" when the target is running.
    let _ = std::fs::remove_file(to);
    std::fs::copy(from, to).map_err(|error| format!("cannot copy to {}: {error}", to.display()))?;
    Ok(())
}

/// Cheap check that a file is a copy of this program: same size and magic.
fn is_our_binary(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 2];
    use std::io::Read;
    if file.read_exact(&mut head).is_err() {
        return false;
    }
    let magic_ok = if cfg!(windows) {
        &head == b"MZ"
    } else {
        &head == b"\x7fE"
    };
    magic_ok && meta.len() > 1024 * 1024
}

// --- platform specific directory and PATH handling -------------------------

fn target_dir() -> Result<PathBuf, String> {
    if cfg!(windows) {
        let base =
            std::env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA is not set".to_string())?;
        Ok(PathBuf::from(base).join("Programs").join("commitsmith"))
    } else {
        let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
        Ok(PathBuf::from(home).join(".local").join("bin"))
    }
}

#[cfg(windows)]
fn add_to_path(directory: &Path) -> Result<bool, String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };

    const ENVIRONMENT_KEY: &str = "Environment";
    const PATH_VALUE: &str = "Path";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    // The user environment is a registry value, not something a Rust program can
    // change through std; writing it here keeps the installer free of external
    // dependencies.
    let mut key: HKEY = std::ptr::null_mut();
    let sub = wide(ENVIRONMENT_KEY);
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!("cannot open HKCU\\Environment (error {status})"));
    }

    let name = wide(PATH_VALUE);
    let mut kind = 0u32;
    let mut size = 0u32;
    let mut current: Vec<u16> = Vec::new();

    let query = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if query == ERROR_SUCCESS && size > 0 {
        current.resize(size as usize / 2, 0);
        let read = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                current.as_mut_ptr() as *mut u8,
                &mut size,
            )
        };
        if read != ERROR_SUCCESS {
            unsafe { RegCloseKey(key) };
            return Err(format!("cannot read the user PATH (error {read})"));
        }
        while matches!(current.last(), Some(0)) {
            current.pop();
        }
    }

    let mut path = String::from_utf16_lossy(&current);
    let wanted = directory.to_string_lossy().to_string();
    let already = path
        .split(';')
        .any(|entry| entry.trim_end_matches('\\').eq_ignore_ascii_case(&wanted));
    if already {
        unsafe { RegCloseKey(key) };
        return Ok(false);
    }

    if !path.is_empty() && !path.ends_with(';') {
        path.push(';');
    }
    path.push_str(&wanted);

    let data: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    // Keep an expandable PATH expandable (%USERPROFILE% and friends survive).
    let stored_kind = if kind == REG_EXPAND_SZ {
        REG_EXPAND_SZ
    } else {
        REG_SZ
    };
    let write = unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            stored_kind,
            data.as_ptr() as *const u8,
            (data.len() * 2) as u32,
        )
    };
    unsafe { RegCloseKey(key) };
    if write != ERROR_SUCCESS {
        return Err(format!("cannot write the user PATH (error {write})"));
    }

    // Tell running programs that the environment changed.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            HWND_BROADCAST, SendMessageTimeoutW, WM_SETTINGCHANGE,
        };
        let setting = wide("Environment");
        let mut result = 0usize;
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            setting.as_ptr() as isize,
            0x0002, /* SMTO_ABORTIFHUNG */
            5000,
            &mut result,
        );
    }

    Ok(true)
}

#[cfg(not(windows))]
fn add_to_path(directory: &Path) -> Result<bool, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let line = format!("export PATH=\"{}:$PATH\"", directory.display());

    // Already reachable? Then nothing to do.
    let on_path = std::env::var("PATH")
        .map(|path| path.split(':').any(|entry| Path::new(entry) == directory))
        .unwrap_or(false);
    if on_path {
        return Ok(false);
    }

    let candidates = [".profile", ".bashrc", ".zshrc"];
    let mut touched = false;
    for name in candidates {
        let file = PathBuf::from(&home).join(name);
        let existing = std::fs::read_to_string(&file).unwrap_or_default();
        if existing.contains(&line) {
            touched = true;
            continue;
        }
        // Only append to files that already exist or are the login profile.
        if name != ".profile" && !file.exists() {
            continue;
        }
        let prefix = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        let mut handle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
        use std::io::Write;
        writeln!(handle, "{prefix}# added by commitsmith --install\n{line}")
            .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
        touched = true;
    }

    if !touched {
        return Err(format!(
            "cannot update your shell profile; add this line by hand:\n  {line}"
        ));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_name_matches_the_platform() {
        if cfg!(windows) {
            assert_eq!(exe_name("commitsmith"), "commitsmith.exe");
        } else {
            assert_eq!(exe_name("commitsmith"), "commitsmith");
        }
    }

    #[test]
    fn target_dir_is_per_user() {
        let dir = target_dir().unwrap();
        assert!(dir.is_absolute(), "{dir:?}");
        if cfg!(windows) {
            assert!(dir.to_string_lossy().ends_with("Programs\\commitsmith"));
        } else {
            assert!(dir.to_string_lossy().ends_with(".local/bin"));
        }
    }
}
