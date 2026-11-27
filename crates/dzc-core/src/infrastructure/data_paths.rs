//! Shared selection of desktop and CLI storage, independent of executable location.
use std::path::{Path, PathBuf};

const DATABASE_NAME: &str = "clipboard.db";
const REDIRECT_NAME: &str = "datapath.txt";

pub fn portable_data_dir(executable_dir: Option<&Path>) -> Option<PathBuf> {
    executable_dir
        .map(|dir| dir.join("data"))
        .filter(|dir| dir.is_dir())
}

pub fn select_data_dir(default_dir: &Path, executable_dir: Option<&Path>) -> PathBuf {
    if let Some(portable) = portable_data_dir(executable_dir) {
        return portable;
    }
    redirected_data_dir(default_dir, Path::is_dir)
}

fn redirected_data_dir(default_dir: &Path, usable: fn(&Path) -> bool) -> PathBuf {
    if let Ok(value) = std::fs::read_to_string(default_dir.join(REDIRECT_NAME)) {
        let custom = PathBuf::from(value.trim());
        if !value.trim().is_empty() && usable(&custom) {
            return custom;
        }
    }
    default_dir.to_path_buf()
}

fn profile_exists(directory: &Path, allow_redirect: bool) -> bool {
    directory.join(DATABASE_NAME).exists()
        || (allow_redirect && directory.join(REDIRECT_NAME).exists())
}

fn profile_base<'a>(current: &'a Path, legacy: Option<&'a Path>, current_redirect_counts: bool) -> &'a Path {
    if profile_exists(current, current_redirect_counts) {
        return current;
    }
    legacy.filter(|directory| profile_exists(directory, true)).unwrap_or(current)
}

pub fn cli_database_path(
    default_dir: &Path,
    legacy_dir: &Path,
    executable_dir: Option<&Path>,
) -> PathBuf {
    let base = profile_base(default_dir, Some(legacy_dir), true);
    select_data_dir(base, executable_dir).join(DATABASE_NAME)
}

/// Resolve the pre-WebView probe path without creating or migrating a profile.
pub fn startup_database_path(
    default_dir: Option<&Path>,
    legacy_dir: Option<&Path>,
    executable_dir: Option<&Path>,
) -> Option<PathBuf> {
    // Desktop startup probes used the portable DB only when it already existed.
    if let Some(portable_db) = executable_dir.map(|dir| dir.join("data").join(DATABASE_NAME)).filter(|path| path.exists()) {
        return Some(portable_db);
    }
    let base = profile_base(default_dir?, legacy_dir, false);
    Some(redirected_data_dir(base, Path::exists).join(DATABASE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_then_desktop_keep_the_same_storage_and_redirects() {
        let base = std::env::temp_dir().join(format!(
            "dzc-data-paths-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let current = base.join("current");
        let legacy = base.join("legacy");
        let custom = base.join("custom");
        let binary = base.join("binary");
        for dir in [&current, &legacy, &custom, &binary] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let initial = cli_database_path(&current, &legacy, Some(&binary));
        assert_eq!(
            initial,
            select_data_dir(&current, Some(&binary)).join("clipboard.db")
        );
        std::fs::write(&initial, b"new history").unwrap();
        std::fs::write(legacy.join("clipboard.db"), b"older larger history").unwrap();
        assert_eq!(cli_database_path(&current, &legacy, Some(&binary)), initial);
        std::fs::write(current.join("datapath.txt"), custom.to_str().unwrap()).unwrap();
        assert_eq!(
            cli_database_path(&current, &legacy, Some(&binary)),
            custom.join("clipboard.db")
        );
        assert_eq!(select_data_dir(&current, Some(&binary)), custom);
        std::fs::create_dir_all(binary.join("data")).unwrap();
        assert_eq!(
            cli_database_path(&current, &legacy, Some(&binary)),
            binary.join("data/clipboard.db")
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn invalid_redirects_fall_back_and_legacy_redirects_work_before_migration() {
        let base = std::env::temp_dir().join(format!("dzc-data-legacy-{}", uuid::Uuid::new_v4()));
        let current = base.join("current");
        let legacy = base.join("legacy");
        let custom = base.join("custom");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::write(legacy.join("datapath.txt"), custom.to_str().unwrap()).unwrap();
        assert_eq!(
            cli_database_path(&current, &legacy, None),
            custom.join("clipboard.db")
        );
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(
            current.join("datapath.txt"),
            base.join("missing").to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            cli_database_path(&current, &legacy, None),
            current.join("clipboard.db")
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn startup_probe_preserves_portable_and_legacy_precedence() {
        let base = std::env::temp_dir().join(format!("dzc-startup-paths-{}", uuid::Uuid::new_v4()));
        let current = base.join("current");
        let legacy = base.join("legacy");
        let data = base.join("data");
        for directory in [&current, &legacy, &data] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let redirect = base.join("自定义");
        std::fs::write(&redirect, b"regular file").unwrap();
        std::fs::write(current.join(REDIRECT_NAME), redirect.to_str().unwrap()).unwrap();
        let legacy_db = legacy.join(DATABASE_NAME);
        std::fs::write(&legacy_db, b"legacy database").unwrap();
        let early = || startup_database_path(Some(&current), Some(&legacy), Some(&base)).unwrap();

        assert_eq!(early(), legacy_db);
        assert_eq!(cli_database_path(&current, &legacy, Some(&base)), data.join(DATABASE_NAME));
        let portable_db = data.join(DATABASE_NAME);
        std::fs::write(&portable_db, b"portable database").unwrap();
        assert_eq!(startup_database_path(None, None, Some(&base)), Some(portable_db.clone()));
        std::fs::remove_file(portable_db).unwrap();
        std::fs::write(current.join(DATABASE_NAME), b"current database").unwrap();
        assert_eq!(early(), redirect.join(DATABASE_NAME));
        assert_eq!(cli_database_path(&current, &legacy, None), current.join(DATABASE_NAME));
        std::fs::remove_dir_all(base).unwrap();
    }
}
