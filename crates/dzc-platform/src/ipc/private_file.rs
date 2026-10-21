//! Private local credentials. Contents are never accepted as OS identity.
use std::{fs::File, io::{self, Read, Write}, path::Path};

pub fn read_private(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = open(path, false)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 4096 { return Err(super::denied()); }
    let mut value = Vec::new();
    Read::by_ref(&mut file).take(4097).read_to_end(&mut value)?;
    if value.len() > 4096 { return Err(super::denied()); }
    Ok(value)
}

/// The endpoint owner calls this only after binding its exclusive listener.
/// A fresh protected file is renamed into place; no existing file is opened for
/// writing, so stale links cannot redirect the credential write.
pub fn write_private(path: &Path, value: &[u8]) -> io::Result<()> {
    if value.len() > 4096 { return Err(super::denied()); }
    let temporary = path.with_extension(format!("{}.new", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = open(&temporary, true)?;
        file.write_all(value)?;
        file.sync_all()?;
        drop(file);
        // Windows rename does not replace an existing destination. It is safe
        // to unlink this stale name only while holding the endpoint listener.
        #[cfg(target_os = "windows")]
        match std::fs::remove_file(path) { Ok(()) => (), Err(e) if e.kind() == io::ErrorKind::NotFound => (), Err(e) => return Err(e) }
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() { let _ = std::fs::remove_file(temporary); }
    result
}

#[cfg(target_os = "linux")]
fn open(path: &Path, create: bool) -> io::Result<File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = std::fs::OpenOptions::new().read(true).write(create).create_new(create)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK).open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 { return Err(super::denied()); }
    Ok(file)
}

#[cfg(target_os = "windows")]
fn open(path: &Path, create: bool) -> io::Result<File> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows::{core::PCWSTR, Win32::{Foundation::{LocalFree, HLOCAL}, Security::{Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1}, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES}, Storage::FileSystem::{CreateFileW, CREATE_NEW, OPEN_EXISTING, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_FLAG_OPEN_REPARSE_POINT}}};
    let identity = super::current_identity()?;
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})", identity.user()).encode_utf16().chain(Some(0)).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut descriptor, None).map_err(io::Error::other)?; }
    struct Allocation(*mut std::ffi::c_void);
    impl Drop for Allocation { fn drop(&mut self) { unsafe { let _ = LocalFree(Some(HLOCAL(self.0))); } } }
    let _allocation = Allocation(descriptor.0);
    let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: descriptor.0, bInheritHandle: false.into() };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let access = if create { (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0 } else { FILE_GENERIC_READ.0 };
    let handle = unsafe { CreateFileW(PCWSTR(wide.as_ptr()), access, FILE_SHARE_READ,
        create.then_some(&attributes as *const _), if create { CREATE_NEW } else { OPEN_EXISTING }, FILE_FLAG_OPEN_REPARSE_POINT, None).map_err(io::Error::other)? };
    // This fresh handle is transferred exactly once to File. Reparse points
    // are opened themselves and rejected rather than followed.
    let file = unsafe { File::from_raw_handle(handle.0) };
    use std::os::windows::fs::MetadataExt;
    if file.metadata()?.file_attributes() & 0x400 != 0 { return Err(super::denied()); }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_bounded_private_regular_files_and_replaced_atomically() {
        let directory = std::env::temp_dir().join(format!("dzc-credentials-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("control.json");
        write_private(&path, b"first").unwrap();
        assert_eq!(read_private(&path).unwrap(), b"first");
        write_private(&path, b"second").unwrap();
        assert_eq!(read_private(&path).unwrap(), b"second");
        assert!(write_private(&path, &[0;4097]).is_err());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(read_private(&path).is_err());
            let link = directory.join("link"); symlink(&path, &link).unwrap(); assert!(read_private(&link).is_err());
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
