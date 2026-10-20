//! Bounded reads from regular local files without waiting for special-file peers.
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

fn open_readonly(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    options.open(path)
}

/// Migration and attachment inputs must be ordinary files at the opened handle.
pub fn open_regular_nofollow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_NOFOLLOW);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Source must be a regular file"));
    }
    Ok(file)
}

pub fn read_regular_nofollow(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = open_regular_nofollow(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_regular_handle(file: File, maximum: usize) -> io::Result<Vec<u8>> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "theme source must be a regular JSON file",
        ));
    }
    let limit = u64::try_from(maximum).unwrap_or(u64::MAX);
    if metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "theme file exceeds the size limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "theme file exceeds the size limit",
        ));
    }
    Ok(bytes)
}

pub fn read_bounded_regular(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "theme source must be a regular JSON file",
        ));
    }
    read_regular_handle(open_readonly(path)?, maximum)
}

pub fn ensure_regular_destination(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "theme export destination must be a regular file",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("dezirclip-theme-file-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn regular_reads_preserve_bytes_and_enforce_the_exact_limit() {
        let directory = Directory::new();
        let path = directory.path("theme.json");
        let raw = b"  {\r\n}\r\n  ";
        std::fs::write(&path, raw).unwrap();
        assert_eq!(read_bounded_regular(&path, raw.len()).unwrap(), raw);
        assert_eq!(
            read_bounded_regular(&path, raw.len() - 1)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            read_bounded_regular(&directory.0, 100).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn export_accepts_only_missing_or_existing_regular_destinations() {
        let directory = Directory::new();
        let path = directory.path("theme.json");
        ensure_regular_destination(&path).unwrap();
        std::fs::write(&path, "existing").unwrap();
        ensure_regular_destination(&path).unwrap();
        assert!(ensure_regular_destination(&directory.0).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing");
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use super::*;
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{symlink, FileTypeExt};
        use std::sync::mpsc;
        use std::time::Duration;

        fn fifo(path: &Path) {
            let name = CString::new(path.as_os_str().as_bytes()).unwrap();
            let result = unsafe { libc::mkfifo(name.as_ptr(), 0o600) };
            assert_eq!(result, 0, "{}", io::Error::last_os_error());
        }

        fn promptly<T: Send + 'static>(action: impl FnOnce() -> T + Send + 'static) -> T {
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(action());
            });
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("special-file access must not wait for a peer")
        }

        #[test]
        fn fifo_and_symlink_to_fifo_fail_without_a_writer() {
            let directory = Directory::new();
            let pipe = directory.path("source.json");
            let link = directory.path("link.json");
            fifo(&pipe);
            symlink(&pipe, &link).unwrap();
            for path in [pipe.clone(), link.clone()] {
                let error = promptly(move || read_bounded_regular(&path, 100)).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            }
            assert!(ensure_regular_destination(&pipe).is_err());
            assert!(ensure_regular_destination(&link).is_err());
            assert!(std::fs::metadata(&pipe).unwrap().file_type().is_fifo());
        }

        #[test]
        fn strict_attachment_inputs_reject_special_files_and_replaced_paths_promptly() {
            let directory = Directory::new();
            let pipe = directory.path("image.png");
            fifo(&pipe);
            let read_path = pipe.clone();
            assert!(promptly(move || read_regular_nofollow(&read_path)).is_err());
            let regular = directory.path("ordinary.png");
            let link = directory.path("alias.png");
            std::fs::write(&regular, b"ordinary").unwrap();
            symlink(&regular, &link).unwrap();
            assert!(open_regular_nofollow(&link).is_err());
            assert_eq!(read_regular_nofollow(&regular).unwrap(), b"ordinary");
            std::fs::remove_file(&regular).unwrap();
            fifo(&regular);
            let opened = regular.clone();
            assert!(promptly(move || open_regular_nofollow(&opened)).is_err());
        }

        #[test]
        fn file_type_is_checked_on_the_opened_handle_after_path_replacement() {
            let directory = Directory::new();
            let path = directory.path("replaced.json");
            fifo(&path);
            let opened_path = path.clone();
            let handle = promptly(move || open_readonly(&opened_path)).unwrap();
            std::fs::remove_file(&path).unwrap();
            std::fs::write(&path, "regular replacement").unwrap();
            assert!(std::fs::metadata(&path).unwrap().is_file());
            assert_eq!(
                read_regular_handle(handle, 100).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );

            let regular = open_readonly(&path).unwrap();
            std::fs::remove_file(&path).unwrap();
            fifo(&path);
            assert_eq!(
                read_regular_handle(regular, 100).unwrap(),
                b"regular replacement"
            );
        }

        #[test]
        fn regular_symlinks_work_but_socket_sources_and_symlink_exports_are_rejected() {
            let directory = Directory::new();
            let file = directory.path("theme.json");
            let link = directory.path("theme-link.json");
            std::fs::write(&file, "{}").unwrap();
            symlink(&file, &link).unwrap();
            assert_eq!(read_bounded_regular(&link, 2).unwrap(), b"{}");
            assert!(ensure_regular_destination(&link).is_err());
            let socket = directory.path("theme.socket");
            let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            let read_path = socket.clone();
            assert!(promptly(move || read_bounded_regular(&read_path, 100)).is_err());
            assert!(ensure_regular_destination(&socket).is_err());
        }
    }
}
