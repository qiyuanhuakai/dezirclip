use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub struct PreviewFile {
    pub canonical_path: PathBuf,
    pub metadata: Metadata,
    file: File,
    max_bytes: u64,
}

fn validate_regular(metadata: &Metadata, max_bytes: u64) -> io::Result<()> {
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Preview source must be a non-empty regular file within the input limit",
        ));
    }
    Ok(())
}

fn validate_unchanged(before: &Metadata, after: &Metadata, max_bytes: u64) -> io::Result<()> {
    validate_regular(after, max_bytes)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Preview source changed"));
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Preview source was replaced"));
        }
    }
    Ok(())
}

fn open_with_hook(
    path: &Path,
    max_bytes: u64,
    before_open: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<PreviewFile> {
    let canonical_path = path.canonicalize()?;
    let before = canonical_path.metadata()?;
    validate_regular(&before, max_bytes)?;
    before_open(&canonical_path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A path can become a FIFO or symlink after the regular-file precheck.
        // Nonblocking open reaches the handle check without waiting for a writer.
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY);
    }
    let file = options.open(&canonical_path)?;
    let metadata = file.metadata()?;
    validate_unchanged(&before, &metadata, max_bytes)?;
    Ok(PreviewFile { canonical_path, metadata, file, max_bytes })
}

pub fn open_regular_file(path: &Path, max_bytes: u64) -> io::Result<PreviewFile> {
    open_with_hook(path, max_bytes, |_| Ok(()))
}

impl PreviewFile {
    pub fn read_checked(mut self) -> io::Result<Vec<u8>> {
        let length = usize::try_from(self.metadata.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Preview source is too large"))?;
        let mut bytes = vec![0; length];
        self.file.read_exact(&mut bytes)?;
        let mut extra = [0u8; 1];
        if self.file.read(&mut extra)? != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Preview source grew while reading"));
        }
        let current = self.file.metadata()?;
        validate_unchanged(&self.metadata, &current, self.max_bytes)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    #[cfg(target_os = "linux")]
    use std::sync::mpsc;
    #[cfg(target_os = "linux")]
    use std::time::Duration;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dezirclip-preview-file-{}-{}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn regular_files_read_and_replacements_between_precheck_and_open_are_rejected() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.png");
        std::fs::write(&path, b"original").unwrap();
        assert_eq!(open_regular_file(&path, 1024).unwrap().read_checked().unwrap(), b"original");
        let result = open_with_hook(&path, 1024, |path| {
            std::fs::remove_file(path)?;
            std::fs::write(path, b"replacement with a different size")
        });
        assert!(result.is_err());
    }

    #[test]
    fn changing_a_file_after_open_is_rejected_before_decode() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.png");
        std::fs::write(&path, b"original").unwrap();
        let source = open_regular_file(&path, 1024).unwrap();
        std::fs::write(&path, b"changed size").unwrap();
        assert_eq!(source.read_checked().unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn shrinking_a_file_after_open_returns_an_incomplete_read_error() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.png");
        std::fs::write(&path, b"original").unwrap();
        let source = open_regular_file(&path, 1024).unwrap();
        std::fs::write(&path, b"x").unwrap();
        assert_eq!(source.read_checked().unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[cfg(target_os = "linux")]
    fn make_fifo(path: &Path) -> io::Result<()> {
        let status = std::process::Command::new("mkfifo").arg("--").arg(path).status()?;
        if status.success() { Ok(()) } else { Err(io::Error::other("mkfifo failed")) }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fifo_without_writer_is_rejected_without_waiting() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.png");
        make_fifo(&path).unwrap();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = open_regular_file(&path, 1024).is_err();
            sender.send(result).unwrap();
            drop(directory);
        });
        assert!(receiver.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn replacement_with_fifo_after_precheck_does_not_wait_for_writer() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.png");
        std::fs::write(&path, b"regular").unwrap();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = open_with_hook(&path, 1024, |path| {
                std::fs::remove_file(path)?;
                make_fifo(path)
            });
            sender.send(result.is_err()).unwrap();
            drop(directory);
        });
        assert!(receiver.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn replacing_with_same_size_and_mtime_still_changes_file_identity() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.png");
        let replacement = directory.0.join("replacement.png");
        std::fs::write(&path, b"original").unwrap();
        std::fs::write(&replacement, b"replaced").unwrap();
        let modified = path.metadata().unwrap().modified().unwrap();
        File::options().write(true).open(&replacement).unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
        assert!(open_with_hook(&path, 1024, |path| std::fs::rename(&replacement, path)).is_err());
    }
}
