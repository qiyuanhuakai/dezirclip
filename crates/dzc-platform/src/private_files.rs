//! Kernel-owned plaintext exports exist only while the initiating Core authorization is current.
use dzc_core::ports::{ClipboardAuthorization, ClipboardFile};
use std::{fs::File, io::Write, path::PathBuf};
static ACTIVE: std::sync::OnceLock<std::sync::Mutex<Vec<ActiveLease>>> = std::sync::OnceLock::new();
struct ActiveLease {
    lease: std::sync::Arc<std::sync::Mutex<Option<Exports>>>,
    clipboard: bool,
}
const MAX_TOTAL: usize = 32 * 1024 * 1024;
pub(crate) struct Exports {
    handles: Vec<File>,
    paths: Vec<String>,
    directory: Option<PathBuf>,
    #[cfg(target_os = "linux")]
    owner: Option<linux_owner::Owner>,
}
impl Exports {
    #[cfg(any(test, target_os = "linux"))]
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
            #[cfg(target_os = "linux")]
            owner: None,
        };
        #[cfg(target_os = "windows")]
        {
            let directory =
                std::env::temp_dir().join(format!("dzc-private-{}", uuid::Uuid::new_v4()));
            create_private_directory(&directory)?;
            exports.directory = Some(directory);
        }
        let mut total = 0usize;
        for (index, file) in files.iter().enumerate() {
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
            let (handle, path) = {
                let _ = index;
                use std::os::fd::FromRawFd;
                let name = std::ffi::CString::new(file.name.as_str())
                    .map_err(|_| "PrivateExportRejected")?;
                let fd = unsafe {
                    libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING)
                };
                if fd < 0 {
                    return Err("PrivateExportRejected".into());
                }
                let handle = unsafe { File::from_raw_fd(fd) };
                let path = String::new();
                (handle, path)
            };
            #[cfg(target_os = "windows")]
            let (handle, path) = {
                use std::os::windows::fs::OpenOptionsExt;
                let directory = exports
                    .directory
                    .as_ref()
                    .ok_or("PrivateExportRejected")?
                    .join(index.to_string());
                create_private_directory(&directory)?;
                let path = directory.join(&file.name);
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
            exports.handles.push(handle);
            exports.paths.push(path);
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            // The new owner has only empty files until its never-before-used PID is approved.
            let owner = linux_owner::Owner::start(&exports.handles)?;
            exports.paths = exports
                .handles
                .iter()
                .map(|file| format!("/proc/{}/fd/{}", owner.pid(), file.as_raw_fd()))
                .collect();
            exports.owner = Some(owner);
        }
        for (handle, file) in exports.handles.iter_mut().zip(&files) {
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
        }
        Ok(exports)
    }
    pub(crate) fn stop(&mut self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        if let Some(owner) = &mut self.owner {
            owner.stop()?;
        }
        Ok(())
    }
    fn modified(&self, index: usize) -> Result<std::time::SystemTime, String> {
        self.handles
            .get(index)
            .ok_or("PrivateExportRejected")?
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|_| "PrivateExportRejected".into())
    }
    fn read(&self, index: usize) -> Result<Vec<u8>, String> {
        let file = self.handles.get(index).ok_or("PrivateExportRejected")?;
        let size = file.metadata().map_err(|_| "PrivateExportRejected")?.len();
        if size > 16 * 1024 * 1024 {
            return Err("PrivateExportRejected".into());
        }
        let mut bytes = vec![0; size as usize];
        let mut offset = 0usize;
        while offset < bytes.len() {
            #[cfg(target_os = "linux")]
            let count = {
                use std::os::unix::fs::FileExt;
                file.read_at(&mut bytes[offset..], offset as u64)
            };
            #[cfg(target_os = "windows")]
            let count = {
                use std::os::windows::fs::FileExt;
                file.seek_read(&mut bytes[offset..], offset as u64)
            };
            let count = count.map_err(|_| "PrivateExportRejected")?;
            if count == 0 {
                return Err("PrivateExportRejected".into());
            }
            offset += count;
        }
        Ok(bytes)
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
        {
            let mut active = ACTIVE
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            active.retain(|entry| {
                entry
                    .lease
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_some()
            });
            active.push(ActiveLease {
                lease: lease.clone(),
                clipboard: !writable,
            });
        }
        if !authorization.is_current() {
            let _ = close_exports(
                &mut lease
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
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
            while close_exports(
                &mut owned
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            )
            .is_err()
            {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
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
    pub(crate) fn modified(&self, index: usize) -> Result<std::time::SystemTime, String> {
        if !self.authorization.is_current() {
            return Err("PrivacyEpochChanged".into());
        }
        self.lease
            .lock()
            .map_err(|_| "PrivateExportRejected")?
            .as_ref()
            .ok_or("PrivacyEpochChanged")?
            .modified(index)
    }
    pub(crate) fn read(&self, index: usize) -> Result<Vec<u8>, String> {
        if !self.authorization.is_current() {
            return Err("PrivacyEpochChanged".into());
        }
        let result = self
            .lease
            .lock()
            .map_err(|_| "PrivateExportRejected")?
            .as_ref()
            .ok_or("PrivacyEpochChanged")?
            .read(index)?;
        if !self.authorization.is_current() {
            return Err("PrivacyEpochChanged".into());
        }
        Ok(result)
    }
    pub(crate) fn retain(mut self) {
        self.retained = true;
    }
    pub(crate) fn close(&mut self) -> Result<(), String> {
        let mut lease = self
            .lease
            .lock()
            .map_err(|_| "PrivateExportRevocationUnconfirmed")?;
        close_exports(&mut lease)
    }
}
impl Drop for BoundExport {
    fn drop(&mut self) {
        if !self.retained {
            let _ = close_exports(
                &mut self
                    .lease
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }
    }
}

fn close_exports(lease: &mut Option<Exports>) -> Result<(), String> {
    if let Some(exports) = lease.as_mut() {
        exports.stop()?;
    }
    lease.take();
    Ok(())
}
pub(crate) fn revoke() -> Result<(), String> {
    revoke_matching(false)
}
pub(crate) fn revoke_clipboard() -> Result<(), String> {
    revoke_matching(true)
}
fn revoke_matching(clipboard_only: bool) -> Result<(), String> {
    let Some(active) = ACTIVE.get() else {
        return Ok(());
    };
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut failed = false;
    active.retain(|entry| {
        let mut lease = entry
            .lease
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !clipboard_only || entry.clipboard {
            if close_exports(&mut lease).is_err() {
                failed = true;
            }
        }
        lease.is_some()
    });
    if failed {
        Err("PrivateExportRevocationUnconfirmed".into())
    } else {
        Ok(())
    }
}

impl Drop for Exports {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        self.owner.take();
        self.handles.clear();
        if let Some(directory) = self.directory.take() {
            if let Ok(children) = std::fs::read_dir(&directory) {
                for child in children.flatten() {
                    let _ = std::fs::remove_dir(child.path());
                }
            }
            let _ = std::fs::remove_dir(directory);
        }
    }
}
#[cfg(target_os = "linux")]
#[path = "private_file_owner.rs"]
mod linux_owner;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn owner_descriptor_table_excludes_the_hosts_other_files() {
        let unrelated = std::fs::File::open("/proc/self/status").unwrap();
        let exports = Exports::create(vec![ClipboardFile {
            name: "synthetic.txt".into(),
            bytes: b"one member".to_vec(),
        }])
        .unwrap();
        let path = exports.paths().remove(0);
        let pid = path.split('/').nth(2).unwrap();
        let descriptors: Vec<_> = std::fs::read_dir(format!("/proc/{pid}/fd"))
            .unwrap()
            .collect();
        assert_eq!(descriptors.len(), 2, "one member and one control pipe");
        use std::os::fd::AsRawFd;
        assert!(std::fs::read_link(format!("/proc/{pid}/fd/{}", unrelated.as_raw_fd())).is_err());
    }
    #[test]
    fn external_editor_reads_the_bound_handle_and_revoke_stops_following_it() {
        use dzc_core::api::{
            HistoryService, PrivacySessionCommand, SensitiveAccess,
        };
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let entry = serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":"editor A","preview":"editor A","source_app":"Synthetic","timestamp":1,"is_pinned":false})).unwrap();
        let id = history.save(&entry, None).unwrap();
        let permit = history
            .privacy()
            .authorize_selected_copy(
                id,
                None,
                SensitiveAccess::Denied,
                None,
                1,
            )
            .unwrap();
        let export = history.external_files_authorized(id, &permit, 1).unwrap();
        let original = BoundExport::new(export.files, export.authorization).unwrap();
        let old_path = original.paths().unwrap().remove(0);
        std::fs::write(&old_path, b"edited A").unwrap();
        assert_eq!(original.read(0).unwrap(), b"edited A");
        history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
            .unwrap();
        assert!(original.read(0).is_err());
        revoke().unwrap();
        let next = Exports::create(vec![ClipboardFile {
            name: "DezirClip.txt".into(),
            bytes: b"editor B".to_vec(),
        }])
        .unwrap();
        assert!(original.read(0).is_err());
        assert!(original.modified(0).is_err());
        assert!(std::fs::read(&old_path).is_err());
        assert_eq!(next.read(0).unwrap(), b"editor B");
    }
    #[test]
    fn revoked_export_never_aliases_a_later_export_in_this_host() {
        let first = Exports::create(vec![ClipboardFile {
            name: "same.txt".into(),
            bytes: b"export A".to_vec(),
        }])
        .unwrap();
        let old_path = first.paths().remove(0);
        drop(first);
        let second = Exports::create(vec![ClipboardFile {
            name: "same.txt".into(),
            bytes: b"export B".to_vec(),
        }])
        .unwrap();
        assert_ne!(old_path, second.paths()[0]);
        assert!(std::fs::read(&old_path).is_err());
        assert_eq!(std::fs::read(&second.paths()[0]).unwrap(), b"export B");
        assert_eq!(second.read(0).unwrap(), b"export B");
    }
    #[test]
    fn equal_and_case_equivalent_basenames_preserve_every_member() {
        let names = ["same.txt", "same.txt", "SAME.TXT"];
        let exports = Exports::create(
            names
                .iter()
                .enumerate()
                .map(|(index, name)| ClipboardFile {
                    name: (*name).into(),
                    bytes: vec![index as u8],
                })
                .collect(),
        )
        .unwrap();
        let paths = exports.paths();
        assert_eq!(paths.len(), names.len());
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(std::fs::read(path).unwrap(), vec![index as u8]);
            assert_eq!(exports.read(index).unwrap(), vec![index as u8]);
        }
        drop(exports);
        for path in paths {
            assert!(std::fs::read(path).is_err());
        }
    }
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
        assert!(!paths[0].starts_with(&format!("/proc/{}/fd/", std::process::id())));
        drop(exports);
        assert!(!std::path::Path::new(&paths[0]).exists());
    }
}
