use std::path::{Path, PathBuf};

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
    let value = path
        .to_string_lossy()
        .trim_end_matches(std::path::MAIN_SEPARATOR)
        .to_string();
    if cfg!(target_os = "windows") {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

#[cfg(target_os = "linux")]
pub fn add_cli_to_path(cli_path: &Path) -> Result<CliPathInstallResult, String> {
    let home = dirs::home_dir().ok_or_else(|| "home directory not found".to_string())?;
    let path_value = std::env::var("PATH").unwrap_or_default();
    add_linux_cli_to_path(cli_path, &home, &path_value)
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

    let link_path = bin_dir.join("tiez-c");
    let mut already_linked = false;
    if link_path.exists() {
        let existing = std::fs::canonicalize(&link_path)
            .map_err(|e| format!("read existing {} failed: {e}", link_path.display()))?;
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

    let current: String = env.get_value("Path").unwrap_or_default();
    let already_linked = path_contains_entry(&current, dir);
    if !already_linked {
        let updated = append_path_entry(&current, dir)?;
        env.set_value("Path", &updated)
            .map_err(|e| format!("write user PATH failed: {e}"))?;
        let process_path = append_path_entry(&std::env::var("PATH").unwrap_or_default(), dir)?;
        std::env::set_var("PATH", process_path);
    }

    Ok(CliPathInstallResult {
        installed_path: cli_path.to_path_buf(),
        path_entry: dir.to_path_buf(),
        already_linked,
        requires_new_terminal: !already_linked,
    })
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
        let entry = PathBuf::from("/tmp/tiez-bin");
        let path_value = std::env::join_paths([PathBuf::from("/usr/bin"), entry.clone()])
            .expect("path value")
            .to_string_lossy()
            .to_string();

        assert!(path_contains_entry(&path_value, &entry));
    }

    #[test]
    fn test_append_path_entry_preserves_existing_entries() {
        let first = PathBuf::from("/usr/bin");
        let second = PathBuf::from("/tmp/tiez-bin");
        let path_value = std::env::join_paths([first.clone()])
            .expect("path value")
            .to_string_lossy()
            .to_string();

        let updated = append_path_entry(&path_value, &second).expect("updated path");
        assert!(path_contains_entry(&updated, &first));
        assert!(path_contains_entry(&updated, &second));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_linux_add_cli_to_path_creates_local_bin_link() {
        let base = std::env::temp_dir().join(format!(
            "tiez-cli-path-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let app_dir = base.join("app");
        let home_dir = base.join("home");
        std::fs::create_dir_all(&app_dir).expect("app dir");
        std::fs::create_dir_all(&home_dir).expect("home dir");
        let cli_path = app_dir.join("tiez-c");
        std::fs::write(&cli_path, b"#!/bin/sh\n").expect("cli file");

        let result = add_linux_cli_to_path(&cli_path, &home_dir, "/usr/bin").expect("install path");
        assert_eq!(result.installed_path, home_dir.join(".local/bin/tiez-c"));
        assert_eq!(result.path_entry, home_dir.join(".local/bin"));
        assert!(!result.already_linked);
        assert!(result.requires_new_terminal);

        let result = add_linux_cli_to_path(&cli_path, &home_dir, "/usr/bin").expect("install again");
        assert!(result.already_linked);

        std::fs::remove_dir_all(base).expect("cleanup");
    }
}
