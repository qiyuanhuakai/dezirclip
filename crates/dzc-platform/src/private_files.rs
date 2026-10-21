//! Kernel-owned plaintext exports exist only while the initiating Core authorization is current.
use dzc_core::ports::{ClipboardAuthorization, ClipboardFile};
use std::{fs::File, io::Write, path::PathBuf};
static ACTIVE: std::sync::OnceLock<std::sync::Mutex<Vec<ActiveLease>>> = std::sync::OnceLock::new();
struct ActiveLease {
    lease: std::sync::Weak<std::sync::Mutex<Option<Exports>>>,
    clipboard: bool,
}
const MAX_TOTAL: usize = 32 * 1024 * 1024;
pub(crate) struct Exports {
    handles: Vec<File>,
    paths: Vec<String>,
    directory: Option<PathBuf>,
}
impl Exports {
    pub(crate) fn create(files: Vec<ClipboardFile>) -> Result<Self, String> {
        Self::create_with_access(files, false)
    }
    fn create_with_access(files: Vec<ClipboardFile>, writable: bool) -> Result<Self, String> {
        #[cfg(target_os = "windows")]
        let _ = writable;
        if files.is_empty() || files.len() > 64 {
            return Err("PrivateExportRejected".into());
        }
        let mut exports = Self {
            handles: Vec::new(),
            paths: Vec::new(),
            directory: None,
        };
        #[cfg(target_os = "windows")]
        {
            let directory =
                std::env::temp_dir().join(format!("dzc-private-{}", uuid::Uuid::new_v4()));
            create_private_directory(&directory)?;
            exports.directory = Some(directory);
        }
        let mut total = 0usize;
        for file in files {
            total = total
                .checked_add(file.bytes.len())
                .ok_or("PrivateExportRejected")?;
            if total > MAX_TOTAL
                || file.name.is_empty()
                || file.name.contains(['/', '\\', ':'])
                || file.name.chars().any(char::is_control)
                || matches!(file.name.as_str(), "." | "..")
            {
                return Err("PrivateExportRejected".into());
            }
            #[cfg(target_os = "linux")]
            let (mut handle, path) = {
                use std::os::fd::{AsRawFd, FromRawFd};
                let name =
                    std::ffi::CString::new(file.name).map_err(|_| "PrivateExportRejected")?;
                let fd = unsafe {
                    libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING)
                };
                if fd < 0 {
                    return Err("PrivateExportRejected".into());
                }
                let handle = unsafe { File::from_raw_fd(fd) };
                let path = format!("/proc/{}/fd/{}", std::process::id(), handle.as_raw_fd());
                (handle, path)
            };
            #[cfg(target_os = "windows")]
            let (mut handle, path) = {
                use std::os::windows::fs::OpenOptionsExt;
                let path = exports
                    .directory
                    .as_ref()
                    .ok_or("PrivateExportRejected")?
                    .join(file.name);
                let handle = std::fs::OpenOptions::new()
                    .write(true)
                    .read(true)
                    .create_new(true)
                    .access_mode(0x80000000 | 0x40000000 | 0x00010000)
                    .share_mode(7)
                    .custom_flags(0x04000000)
                    .open(&path)
                    .map_err(|_| "PrivateExportRejected")?;
                (handle, path.to_string_lossy().into_owned())
            };
            handle
                .write_all(&file.bytes)
                .map_err(|_| "PrivateExportRejected")?;
            #[cfg(target_os = "linux")]
            if !writable {
                use std::os::fd::AsRawFd;
                if unsafe {
                    libc::fcntl(
                        handle.as_raw_fd(),
                        libc::F_ADD_SEALS,
                        libc::F_SEAL_WRITE
                            | libc::F_SEAL_GROW
                            | libc::F_SEAL_SHRINK
                            | libc::F_SEAL_SEAL,
                    )
                } < 0
                {
                    return Err("PrivateExportRejected".into());
                }
            }
            exports.handles.push(handle);
            exports.paths.push(path);
        }
        Ok(exports)
    }
    pub(crate) fn paths(&self) -> Vec<String> {
        self.paths.clone()
    }
}
#[cfg(target_os = "windows")]
fn create_private_directory(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
            Security::{
                Authorization::{
                    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                    SDDL_REVISION_1,
                },
                GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
                TOKEN_QUERY, TOKEN_USER,
            },
            Storage::FileSystem::CreateDirectoryW,
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };
    fn rejected<T>(_: T) -> String {
        "PrivateExportRejected".to_owned()
    }
    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    struct Allocation(*mut std::ffi::c_void);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0)));
            }
        }
    }
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(rejected)?;
    }
    let token = OwnedHandle(token);
    let mut length = 0;
    unsafe {
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut length);
    }
    if length == 0 || length > 65_536 {
        return Err("PrivateExportRejected".into());
    }
    let mut storage = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            length,
            &mut length,
        )
        .map_err(rejected)?;
    }
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(user.User.Sid, &mut sid).map_err(rejected)?;
    }
    let _sid = Allocation(sid.0.cast());
    let sid = unsafe { sid.to_string().map_err(rejected)? };
    let sddl: Vec<u16> = format!("D:P(A;OICI;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(rejected)?;
    }
    let _descriptor = Allocation(descriptor.0);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        CreateDirectoryW(PCWSTR(path.as_ptr()), Some(&attributes)).map_err(rejected)?;
    }
    Ok(())
}

pub(crate) struct BoundExport {
    lease: std::sync::Arc<std::sync::Mutex<Option<Exports>>>,
    authorization: ClipboardAuthorization,
    retained: bool,
}
impl BoundExport {
    pub(crate) fn new(
        files: Vec<ClipboardFile>,
        authorization: ClipboardAuthorization,
    ) -> Result<Self, String> {
        Self::with_access(files, authorization, true)
    }
    pub(crate) fn readonly(
        files: Vec<ClipboardFile>,
        authorization: ClipboardAuthorization,
    ) -> Result<Self, String> {
        Self::with_access(files, authorization, false)
    }
    fn with_access(
        files: Vec<ClipboardFile>,
        authorization: ClipboardAuthorization,
        writable: bool,
    ) -> Result<Self, String> {
        if !authorization.is_current() {
            return Err("PrivacyEpochChanged".into());
        }
        let exports = Exports::create_with_access(files, writable)?;
        let lease = std::sync::Arc::new(std::sync::Mutex::new(Some(exports)));
        ACTIVE
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(ActiveLease {
                lease: std::sync::Arc::downgrade(&lease),
                clipboard: !writable,
            });
        if !authorization.is_current() {
            lease
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            return Err("PrivacyEpochChanged".into());
        }
        let owned = lease.clone();
        let check = authorization.clone();
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
            while check.is_current() && std::time::Instant::now() < deadline {
                if owned
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_none()
                {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            owned
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
        });
        Ok(Self {
            lease,
            authorization,
            retained: false,
        })
    }
    pub(crate) fn paths(&self) -> Result<Vec<String>, String> {
        if !self.authorization.is_current() {
            return Err("PrivacyEpochChanged".into());
        }
        self.lease
            .lock()
            .map_err(|_| "PrivateExportRejected".to_owned())?
            .as_ref()
            .map(Exports::paths)
            .ok_or("PrivacyEpochChanged".into())
    }
    pub(crate) fn retain(mut self) {
        self.retained = true;
    }
}
impl Drop for BoundExport {
    fn drop(&mut self) {
        if !self.retained {
            self.lease
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
        }
    }
}

pub(crate) fn revoke() {
    revoke_matching(false);
}
pub(crate) fn revoke_clipboard() {
    revoke_matching(true);
}
fn revoke_matching(clipboard_only: bool) {
    let Some(active) = ACTIVE.get() else {
        return;
    };
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    active.retain(|entry| {
        let Some(lease) = entry.lease.upgrade() else {
            return false;
        };
        if !clipboard_only || entry.clipboard {
            lease
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            false
        } else {
            true
        }
    });
}

impl Drop for Exports {
    fn drop(&mut self) {
        self.handles.clear();
        if let Some(directory) = self.directory.take() {
            let _ = std::fs::remove_dir(directory);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_readers_are_revoked_while_an_existing_reader_can_finish() {
        use std::io::Read;
        let exports = Exports::create(vec![ClipboardFile {
            name: "synthetic.txt".into(),
            bytes: b"shared existing reader".to_vec(),
        }])
        .unwrap();
        let path = exports.paths().remove(0);
        let mut reader = std::fs::File::open(&path).unwrap();
        drop(exports);
        assert!(std::fs::File::open(&path).is_err());
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"shared existing reader");
        drop(reader);
        assert!(!std::path::Path::new(&path).exists());
    }
    #[test]
    fn independent_reader_gets_complete_bytes_and_closed_kernel_lease_leaves_no_file() {
        let exports = Exports::create(vec![ClipboardFile {
            name: "synthetic.txt".into(),
            bytes: b"private synthetic bytes".to_vec(),
        }])
        .unwrap();
        let paths = exports.paths();
        assert_eq!(
            std::fs::read(&paths[0]).unwrap(),
            b"private synthetic bytes"
        );
        #[cfg(target_os = "linux")]
        assert!(paths[0].starts_with(&format!("/proc/{}/fd/", std::process::id())));
        drop(exports);
        assert!(!std::path::Path::new(&paths[0]).exists());
    }
}
