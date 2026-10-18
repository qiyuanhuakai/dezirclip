use std::ffi::OsStr;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn find_cli_on_path_in(path_value: &OsStr, binary_name: &str) -> Option<PathBuf> {
    std::env::split_paths(path_value)
        .map(|entry| entry.join(binary_name))
        .find(|candidate| {
            let Ok(metadata) = candidate.metadata() else {
                return false;
            };
            if !metadata.is_file() {
                return false;
            }
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    return false;
                }
            }
            true
        })
}

pub fn cli_paths_differ(selected: &Path, on_path: &Path) -> bool {
    normalize_path(selected) != normalize_path(on_path)
}

fn parse_cli_version(output: &[u8]) -> Option<String> {
    let output = std::str::from_utf8(output).ok()?;
    let version = output
        .trim()
        .strip_prefix("dzc ")?
        .split_whitespace()
        .next()?;
    if version.len() > 100 || !version.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(version.to_string())
}

pub fn read_cli_version(path: &Path) -> Option<String> {
    let output_path =
        std::env::temp_dir().join(format!("dzc-version-{}.tmp", uuid::Uuid::new_v4()));
    let mut output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&output_path)
        .ok()?;
    let result = read_cli_version_with_output(path, &mut output);
    drop(output);
    let _ = std::fs::remove_file(output_path);
    result
}

fn read_cli_version_with_output(path: &Path, output: &mut std::fs::File) -> Option<String> {
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(output.try_clone().ok()?)
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut bytes = Vec::new();
                output.seek(SeekFrom::Start(0)).ok()?;
                output.take(4096).read_to_end(&mut bytes).ok()?;
                return parse_cli_version(&bytes);
            }
            Ok(None) if started.elapsed() < Duration::from_secs(2) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliPathInstallResult {
    pub installed_path: PathBuf,
    pub path_entry: PathBuf,
    pub already_linked: bool,
    pub requires_new_terminal: bool,
}

pub fn path_contains_entry(path_value: &str, entry: &Path) -> bool {
    let target = normalize_path(entry);
    std::env::split_paths(path_value).any(|candidate| normalize_path(&candidate) == target)
}

pub fn append_path_entry(path_value: &str, entry: &Path) -> Result<String, String> {
    if path_contains_entry(path_value, entry) {
        return Ok(path_value.to_string());
    }

    let mut entries: Vec<PathBuf> = std::env::split_paths(path_value).collect();
    entries.push(entry.to_path_buf());
    std::env::join_paths(entries)
        .map_err(|e| format!("join PATH entries failed: {e}"))
        .map(|value| value.to_string_lossy().to_string())
}

fn normalize_path(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = resolved
        .to_string_lossy()
        .trim_end_matches(std::path::MAIN_SEPARATOR)
        .to_string();
    if cfg!(target_os = "windows") {
        value.replace('/', "\\").to_lowercase()
    } else {
        value
    }
}

#[cfg(target_os = "linux")]
pub fn add_cli_to_path(cli_path: &Path) -> Result<CliPathInstallResult, String> {
    if let Some(message) = linux_path_install_restriction(
        std::env::var_os("FLATPAK_ID").as_deref(),
        Path::new("/.flatpak-info").is_file(),
        std::env::var_os("APPIMAGE").as_deref(),
    ) {
        return Err(message.to_string());
    }
    let home = dirs::home_dir().ok_or_else(|| "home directory not found".to_string())?;
    let path_value = std::env::var("PATH").unwrap_or_default();
    add_linux_cli_to_path(cli_path, &home, &path_value)
}

#[cfg(target_os = "linux")]
fn linux_path_install_restriction(
    flatpak_id: Option<&OsStr>,
    flatpak_info_exists: bool,
    appimage: Option<&OsStr>,
) -> Option<&'static str> {
    if flatpak_id.is_some_and(|value| !value.is_empty()) || flatpak_info_exists {
        return Some("PATH setup is unavailable inside Flatpak. Run: flatpak run --command=dzc io.github.qiyuanhuakai.dezirclip");
    }
    if appimage.is_some_and(|value| !value.is_empty()) {
        return Some("PATH setup is unavailable from an AppImage mount. Install the standalone dzc archive in a persistent directory, or extract dzc to a permanent location and add that directory to PATH.");
    }
    None
}

#[cfg(target_os = "linux")]
fn add_linux_cli_to_path(
    cli_path: &Path,
    home: &Path,
    path_value: &str,
) -> Result<CliPathInstallResult, String> {
    let bin_dir = home.join(".local").join("bin");
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("create {} failed: {e}", bin_dir.display()))?;

    let link_path = bin_dir.join("dzc");
    let mut already_linked = false;
    if std::fs::symlink_metadata(&link_path).is_ok() {
        let existing = std::fs::canonicalize(&link_path).map_err(|e| {
            format!(
                "{} already exists and cannot be resolved; it was left unchanged: {e}",
                link_path.display()
            )
        })?;
        let expected = std::fs::canonicalize(cli_path)
            .map_err(|e| format!("read {} failed: {e}", cli_path.display()))?;
        if existing != expected {
            return Err(format!(
                "{} already exists and points to {}",
                link_path.display(),
                existing.display()
            ));
        }
        already_linked = true;
    } else {
        std::os::unix::fs::symlink(cli_path, &link_path)
            .map_err(|e| format!("link {} failed: {e}", link_path.display()))?;
    }

    Ok(CliPathInstallResult {
        installed_path: link_path,
        path_entry: bin_dir.clone(),
        already_linked,
        requires_new_terminal: !path_contains_entry(&path_value, &bin_dir),
    })
}

#[cfg(target_os = "windows")]
pub fn add_cli_to_path(cli_path: &Path) -> Result<CliPathInstallResult, String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let dir = cli_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", cli_path.display()))?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .or_else(|_| hkcu.create_subkey("Environment").map(|(key, _)| key))
        .map_err(|e| format!("open user Environment failed: {e}"))?;

    let current = match env.get_raw_value("Path") {
        Ok(value) => Some(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("read user PATH failed: {error}")),
    };
    let (updated, already_linked) = windows_registry_path_value(current.as_ref(), dir)?;
    let process_current = std::env::var("PATH").unwrap_or_default();
    let process_already_linked = path_contains_entry(&process_current, dir);
    if !already_linked {
        env.set_raw_value("Path", &updated)
            .map_err(|e| format!("write user PATH failed: {e}"))?;
    }
    std::env::set_var("PATH", append_path_entry(&process_current, dir)?);

    Ok(CliPathInstallResult {
        installed_path: cli_path.to_path_buf(),
        path_entry: dir.to_path_buf(),
        already_linked,
        requires_new_terminal: !already_linked || !process_already_linked,
    })
}

#[cfg(target_os = "windows")]
fn windows_registry_path_value(
    current: Option<&winreg::RegValue>,
    dir: &Path,
) -> Result<(winreg::RegValue, bool), String> {
    windows_registry_path_value_with_lookup(current, dir, |name| std::env::var(name).ok())
}

#[cfg(target_os = "windows")]
fn expand_windows_path_variables(value: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut remaining = value;
    let mut result = String::new();
    while let Some(start) = remaining.find('%') {
        result.push_str(&remaining[..start]);
        let after_start = &remaining[start + 1..];
        let Some(end) = after_start.find('%') else {
            result.push_str(&remaining[start..]);
            return result;
        };
        let name = &after_start[..end];
        if let Some(replacement) = lookup(name) {
            result.push_str(&replacement);
        } else {
            result.push_str(&remaining[start..start + end + 2]);
        }
        remaining = &after_start[end + 1..];
    }
    result.push_str(remaining);
    result
}

#[cfg(target_os = "windows")]
fn windows_registry_path_value_with_lookup(
    current: Option<&winreg::RegValue>,
    dir: &Path,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<(winreg::RegValue, bool), String> {
    use winreg::enums::{REG_EXPAND_SZ, REG_SZ};
    use winreg::types::{FromRegValue, ToRegValue};
    if current.is_some_and(|value| !matches!(value.vtype, REG_SZ | REG_EXPAND_SZ)) {
        return Err("user PATH is not a string; it was left unchanged".to_string());
    }
    let path = current
        .map(String::from_reg_value)
        .transpose()
        .map_err(|error| format!("decode user PATH failed: {error}"))?
        .unwrap_or_default();
    let comparison = if current.is_some_and(|value| value.vtype == REG_EXPAND_SZ) {
        expand_windows_path_variables(&path, lookup)
    } else {
        path.clone()
    };
    let already_linked = path_contains_entry(&comparison, dir);
    let value = if already_linked {
        path
    } else {
        append_path_entry(&path, dir)?
    };
    let mut updated = value.to_reg_value();
    updated.vtype = current
        .map(|value| value.vtype.clone())
        .unwrap_or(REG_EXPAND_SZ);
    Ok((updated, already_linked))
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn add_cli_to_path(_cli_path: &Path) -> Result<CliPathInstallResult, String> {
    Err("unsupported platform".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_contains_entry_matches_existing_path() {
        let entry = PathBuf::from("/tmp/dezirclip-bin");
        let path_value = std::env::join_paths([PathBuf::from("/usr/bin"), entry.clone()])
            .expect("path value")
            .to_string_lossy()
            .to_string();

        assert!(path_contains_entry(&path_value, &entry));
    }

    #[test]
    fn test_append_path_entry_preserves_existing_entries() {
        let first = PathBuf::from("/usr/bin");
        let second = PathBuf::from("/tmp/dezirclip-bin");
        let path_value = std::env::join_paths([first.clone()])
            .expect("path value")
            .to_string_lossy()
            .to_string();

        let updated = append_path_entry(&path_value, &second).expect("updated path");
        assert!(path_contains_entry(&updated, &first));
        assert!(path_contains_entry(&updated, &second));
    }

    #[test]
    fn path_resolution_reports_the_first_copy_without_modifying_either_file() {
        let base = std::env::temp_dir().join(format!("dzc-path-copies-{}", uuid::Uuid::new_v4()));
        let standalone = base.join("standalone");
        let bundled = base.join("bundled");
        for directory in [&standalone, &bundled] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let name = if cfg!(target_os = "windows") {
            "dzc.exe"
        } else {
            "dzc"
        };
        let first = standalone.join(name);
        let second = bundled.join(name);
        std::fs::write(&first, b"standalone copy").unwrap();
        std::fs::write(&second, b"bundled copy").unwrap();
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&first, &second] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let path = std::env::join_paths([&standalone, &bundled]).unwrap();
        assert_eq!(find_cli_on_path_in(&path, name), Some(first.clone()));
        assert!(cli_paths_differ(&second, &first));
        assert!(!cli_paths_differ(&first, &first));
        assert_eq!(std::fs::read(&first).unwrap(), b"standalone copy");
        assert_eq!(std::fs::read(&second).unwrap(), b"bundled copy");
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn cli_version_is_parsed_from_the_executable_output() {
        assert_eq!(
            parse_cli_version(b"dzc 0.1.12\r\n"),
            Some("0.1.12".to_string())
        );
        assert_eq!(
            parse_cli_version(b"dzc 1.0.0-beta.1\n"),
            Some("1.0.0-beta.1".to_string())
        );
        assert!(parse_cli_version(b"not implemented").is_none());
        assert!(parse_cli_version(b"dezirclip 0.1.12").is_none());
        assert!(parse_cli_version(b"dzc unknown").is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn registry_path_keeps_expandable_type_and_environment_references() {
        use winreg::enums::{REG_EXPAND_SZ, REG_SZ};
        use winreg::types::{FromRegValue, ToRegValue};
        let mut current = "%USERPROFILE%\\bin;C:\\Other".to_reg_value();
        current.vtype = REG_EXPAND_SZ;
        let dir = Path::new("C:\\DezirClip");
        let (updated, already) =
            windows_registry_path_value_with_lookup(Some(&current), dir, |_| None).unwrap();
        assert!(!already);
        assert_eq!(updated.vtype, REG_EXPAND_SZ);
        assert_eq!(
            String::from_reg_value(&updated).unwrap(),
            "%USERPROFILE%\\bin;C:\\Other;C:\\DezirClip"
        );
        let (unchanged, already) = windows_registry_path_value_with_lookup(
            Some(&current),
            Path::new("C:\\User\\bin"),
            |name| (name == "USERPROFILE").then(|| "C:\\User".to_string()),
        )
        .unwrap();
        assert!(already);
        assert_eq!(unchanged, current);
        let plain = "C:\\Other".to_reg_value();
        let (updated, _) = windows_registry_path_value(Some(&plain), dir).unwrap();
        assert_eq!(updated.vtype, REG_SZ);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn registry_path_refuses_non_string_values_without_overwriting_them() {
        use winreg::types::ToRegValue;
        let current = 7u32.to_reg_value();
        assert!(windows_registry_path_value(Some(&current), Path::new("C:\\DezirClip")).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_existing_file_or_broken_link_is_left_unchanged() {
        let base = std::env::temp_dir().join(format!("dzc-path-existing-{}", uuid::Uuid::new_v4()));
        let home = base.join("home");
        let local = home.join(".local/bin");
        std::fs::create_dir_all(&local).unwrap();
        let bundled = base.join("bundled-dzc");
        std::fs::write(&bundled, b"bundled").unwrap();
        let existing = local.join("dzc");
        std::fs::write(&existing, b"standalone").unwrap();
        assert!(add_linux_cli_to_path(&bundled, &home, "").is_err());
        assert_eq!(std::fs::read(&existing).unwrap(), b"standalone");
        std::fs::remove_file(&existing).unwrap();
        let missing = base.join("removed-standalone");
        std::os::unix::fs::symlink(&missing, &existing).unwrap();
        assert!(add_linux_cli_to_path(&bundled, &home, "")
            .unwrap_err()
            .contains("left unchanged"));
        assert_eq!(std::fs::read_link(&existing).unwrap(), missing);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_and_temporary_mount_path_installation_has_actionable_refusals() {
        let flatpak = linux_path_install_restriction(
            Some(OsStr::new("io.github.qiyuanhuakai.dezirclip")),
            false,
            None,
        )
        .unwrap();
        assert!(flatpak.contains("flatpak run --command=dzc io.github.qiyuanhuakai.dezirclip"));
        assert!(linux_path_install_restriction(None, true, None)
            .unwrap()
            .contains("Flatpak"));
        let appimage =
            linux_path_install_restriction(None, false, Some(OsStr::new("/tmp/app.AppImage")))
                .unwrap();
        assert!(appimage.contains("standalone dzc archive"));
        assert!(appimage.contains("persistent directory"));
        assert!(linux_path_install_restriction(None, false, None).is_none());
        assert!(
            linux_path_install_restriction(Some(OsStr::new("")), false, Some(OsStr::new("")))
                .is_none()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_linux_add_cli_to_path_creates_local_bin_link() {
        let base = std::env::temp_dir().join(format!("dzc-cli-path-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let app_dir = base.join("app");
        let home_dir = base.join("home");
        std::fs::create_dir_all(&app_dir).expect("app dir");
        std::fs::create_dir_all(&home_dir).expect("home dir");
        let cli_path = app_dir.join("dzc");
        std::fs::write(&cli_path, b"#!/bin/sh\n").expect("cli file");

        let result = add_linux_cli_to_path(&cli_path, &home_dir, "/usr/bin").expect("install path");
        assert_eq!(result.installed_path, home_dir.join(".local/bin/dzc"));
        assert_eq!(result.path_entry, home_dir.join(".local/bin"));
        assert!(!result.already_linked);
        assert!(result.requires_new_terminal);

        let result =
            add_linux_cli_to_path(&cli_path, &home_dir, "/usr/bin").expect("install again");
        assert!(result.already_linked);

        std::fs::remove_dir_all(base).expect("cleanup");
    }
}
