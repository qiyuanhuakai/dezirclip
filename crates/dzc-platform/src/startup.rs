use std::{fs::File, path::Path};

// SQLite reserves these bytes in the main database for its rollback and WAL
// ownership locks. Locking the whole range excludes both VFS lock protocols.
const SQLITE_LOCK_START: u32 = 0x4000_0000;
const SQLITE_LOCK_LENGTH: u32 = 512;

pub fn probe_disable_webview_gpu(path: &Path) -> bool {
    dzc_core::api::probe_startup_disable_webview_gpu(path, lock_database)
}

#[cfg(target_os = "linux")]
fn lock_database(path: &Path) -> Option<File> {
    use std::os::fd::AsRawFd;
    // Linux requires a writable descriptor for a write lock. Opening this
    // existing file only supplies the handle used by fcntl.
    let file = File::options().read(true).write(true).open(path).ok()?;
    // A zeroed flock is initialized below and points to live storage for fcntl.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as libc::c_short;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    lock.l_start = SQLITE_LOCK_START.into();
    lock.l_len = SQLITE_LOCK_LENGTH.into();
    // An OFD lock conflicts with SQLite's process locks even in the same
    // process. Dropping this handle releases its lock.
    (unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_SETLK, &lock) } == 0).then_some(file)
}

#[cfg(target_os = "windows")]
fn lock_database(path: &Path) -> Option<File> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::LockFile};

    let file = File::open(path).ok()?;
    // The handle remains open until after the read. A conflicting byte-range
    // lock returns immediately and no bytes from that range are accessed.
    unsafe { LockFile(HANDLE(file.as_raw_handle()), SQLITE_LOCK_START, 0, SQLITE_LOCK_LENGTH, 0) }.ok()?;
    Some(file)
}
