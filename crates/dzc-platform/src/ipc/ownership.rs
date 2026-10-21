//! A native capture lease is held by each actual worker, not only its Host.
use std::{io, sync::Arc};

pub struct CaptureLease {
    #[cfg(target_os = "windows")]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(target_os = "linux")]
    _file: std::fs::File,
}

impl CaptureLease {
    /// Windows: one lease per verified user/token session. Linux: conservatively
    /// one per UID across graphical sessions, independent of display labels or
    /// database paths. Aliases cannot create a second clipboard capturer.
    pub fn acquire() -> io::Result<Arc<Self>> {
        #[cfg(target_os = "windows")]
        { windows_lease() }
        #[cfg(target_os = "linux")]
        { linux_lease() }
    }
}

#[cfg(target_os = "windows")]
// The kernel semaphore has no owning thread. Its sole handle is shared only
// through Arc and released/closed exactly once by the final worker reference.
unsafe impl Send for CaptureLease {}
#[cfg(target_os = "windows")]
unsafe impl Sync for CaptureLease {}

#[cfg(target_os = "windows")]
fn windows_lease() -> io::Result<Arc<CaptureLease>> {
    use windows::{core::PCWSTR, Win32::{Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL, WAIT_OBJECT_0},
        Security::{Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1}, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
        System::Threading::{CreateSemaphoreW, WaitForSingleObject}}};
    let identity = super::current_identity()?;
    let name: Vec<u16> = format!("Local\\DezirClipCapture-{}-{}", identity.user(), identity.session()).encode_utf16().chain(Some(0)).collect();
    let dacl: Vec<u16> = format!("D:P(A;;GA;;;{})", identity.user()).encode_utf16().chain(Some(0)).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(dacl.as_ptr()), SDDL_REVISION_1, &mut descriptor, None).map_err(|_| super::denied())?; }
    let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: descriptor.0, bInheritHandle: false.into() };
    let result = unsafe { CreateSemaphoreW(Some(&attributes), 1, 1, PCWSTR(name.as_ptr())) };
    unsafe { let _ = LocalFree(Some(HLOCAL(descriptor.0))); }
    let handle: HANDLE = result.map_err(|_| super::denied())?;
    if unsafe { WaitForSingleObject(handle, 0) } != WAIT_OBJECT_0 {
        unsafe { let _ = CloseHandle(handle); }
        return Err(io::Error::from(io::ErrorKind::WouldBlock));
    }
    Ok(Arc::new(CaptureLease { handle }))
}

#[cfg(target_os = "windows")]
impl Drop for CaptureLease {
    fn drop(&mut self) {
        // A semaphore can be released by the last worker's thread; a mutex
        // acquired on the Host thread cannot safely provide this Arc lifetime.
        unsafe {
            let _ = windows::Win32::System::Threading::ReleaseSemaphore(self.handle, 1, None);
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_lease() -> io::Result<Arc<CaptureLease>> {
    use std::{fs, os::{fd::AsRawFd, unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt}}, path::Path};
    let identity = super::current_identity()?;
    let directory = Path::new("/tmp").join(format!("dezirclip-capture-{}", identity.user()));
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => (), Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (), Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if !metadata.is_dir() || metadata.uid().to_string() != identity.user() || metadata.mode() & 0o077 != 0 { return Err(super::denied()); }
    let file = fs::OpenOptions::new().read(true).write(true).create(true).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(directory.join("all-graphical-sessions.lock"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.uid().to_string() != identity.user() || metadata.mode() & 0o077 != 0 { return Err(super::denied()); }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 { return Err(io::Error::from(io::ErrorKind::WouldBlock)); }
    Ok(Arc::new(CaptureLease { _file: file }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_worker_reference_keeps_lease_when_host_reference_is_dropped() {
        let host = CaptureLease::acquire().unwrap();
        let worker = host.clone();
        drop(host);
        assert!(CaptureLease::acquire().is_err());
        std::thread::spawn(move || drop(worker)).join().unwrap();
        assert!(CaptureLease::acquire().is_ok());
    }
}
