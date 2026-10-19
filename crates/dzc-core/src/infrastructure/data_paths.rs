//! Shared selection of desktop and CLI storage, independent of executable location.
use std::path::{Path, PathBuf};

pub fn portable_data_dir(executable_dir: Option<&Path>) -> Option<PathBuf> {
    executable_dir
        .map(|dir| dir.join("data"))
        .filter(|dir| dir.is_dir())
}

pub fn select_data_dir(default_dir: &Path, executable_dir: Option<&Path>) -> PathBuf {
    if let Some(portable) = portable_data_dir(executable_dir) {
        return portable;
    }
    if let Ok(value) = std::fs::read_to_string(default_dir.join("datapath.txt")) {
        let custom = PathBuf::from(value.trim());
        if !value.trim().is_empty() && custom.is_dir() {
            return custom;
        }
    }
    default_dir.to_path_buf()
}

pub fn cli_database_path(
    default_dir: &Path,
    legacy_dir: &Path,
    executable_dir: Option<&Path>,
) -> PathBuf {
    let current_exists =
        default_dir.join("clipboard.db").exists() || default_dir.join("datapath.txt").exists();
    let base = if !current_exists
        && (legacy_dir.join("clipboard.db").exists() || legacy_dir.join("datapath.txt").exists())
    {
        legacy_dir
    } else {
        default_dir
    };
    select_data_dir(base, executable_dir).join("clipboard.db")
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
}
