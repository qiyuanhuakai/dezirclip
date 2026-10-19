//! Stored attachment decoding and atomic backup file replacement.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn path_from_uri(value: &str) -> Result<String, String> {
    let Some(uri) = value.strip_prefix("file://") else {
        return Ok(value.to_owned());
    };
    let raw = if uri.starts_with('/') || uri.as_bytes().get(1) == Some(&b':') {
        uri.to_owned()
    } else {
        let (host, path) = uri.split_once('/').ok_or("file URI has no path")?;
        if host.eq_ignore_ascii_case("localhost") {
            format!("/{path}")
        } else {
            format!("//{host}/{path}")
        }
    };
    let path = urlencoding::decode(&raw)
        .map_err(|error| error.to_string())?
        .into_owned();
    #[cfg(target_os = "windows")]
    let path = if path.starts_with('/') && path.chars().nth(2) == Some(':') {
        path[1..].to_owned()
    } else {
        path
    };
    Ok(path)
}

pub fn image_bytes(content: &str) -> Result<Vec<u8>, String> {
    if content.starts_with("data:image/") {
        let (_, payload) = content.split_once(',').ok_or("invalid image data URL")?;
        STANDARD.decode(payload).map_err(|error| error.to_string())
    } else {
        std::fs::read(path_from_uri(content)?).map_err(|error| format!("read image: {error}"))
    }
}

pub fn resolved_path(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path.canonicalize().map_err(|error| error.to_string());
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    let mut base = absolute.as_path();
    let mut suffix = Vec::new();
    while !base.exists() {
        suffix.push(base.file_name().ok_or("invalid backup path")?.to_owned());
        base = base.parent().ok_or("invalid backup path")?;
    }
    let mut result = base.canonicalize().map_err(|error| error.to_string())?;
    for component in suffix.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
}

pub fn write_backup(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".dezirclip-backup-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_is_complete_and_failed_rename_preserves_existing_target() {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-backup-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let backup = directory.join("backup.json");
        std::fs::write(&backup, b"previous backup").unwrap();
        let content = "完整备份\r\n".repeat(2000);
        write_backup(&backup, content.as_bytes()).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), content.as_bytes());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let existing_directory = directory.join("keep");
        std::fs::create_dir(&existing_directory).unwrap();
        std::fs::write(existing_directory.join("original"), b"keep").unwrap();
        assert!(write_backup(&existing_directory, b"new backup").is_err());
        assert_eq!(
            std::fs::read(existing_directory.join("original")).unwrap(),
            b"keep"
        );
        assert!(!std::fs::read_dir(&directory).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".dezirclip-backup-")));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
