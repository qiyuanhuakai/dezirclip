//! Last-chance native exception evidence. No clipboard data or memory dumps.
#[cfg(target_os = "windows")]
mod windows {
    use std::{fmt::{self, Write}, fs::OpenOptions, os::windows::io::IntoRawHandle, path::Path, sync::atomic::{AtomicBool, AtomicPtr, Ordering}};
    use ::windows::{core::PCWSTR, Win32::{Foundation::{HANDLE, HMODULE}, Storage::FileSystem::{FlushFileBuffers, WriteFile}, System::{Diagnostics::Debug::{SetUnhandledExceptionFilter, EXCEPTION_POINTERS}, LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT}, Threading::{GetCurrentProcessId, GetCurrentThreadId}}}};

    static FILE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
    static ENTERED: AtomicBool = AtomicBool::new(false);
    static PREVIOUS: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
    struct Buffer { bytes: [u8; 1024], len: usize }
    impl fmt::Write for Buffer {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            let len = value.len().min(self.bytes.len() - self.len);
            self.bytes[self.len..self.len + len].copy_from_slice(&value.as_bytes()[..len]);
            self.len += len;
            Ok(())
        }
    }
    unsafe extern "system" fn record(info: *const EXCEPTION_POINTERS) -> i32 {
        let handle = FILE.load(Ordering::Acquire);
        if handle.is_null() || info.is_null() || ENTERED.swap(true, Ordering::AcqRel) { return 0; }
        let record = (*info).ExceptionRecord;
        if !record.is_null() {
            let record = &*record;
            let mut output = Buffer { bytes: [0; 1024], len: 0 };
            let time = ::windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime();
            let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
            let epoch_ms = ticks.saturating_sub(116444736000000000) / 10000;
            let _ = write!(output, "{epoch_ms} pid={} tid={} version={} code=0x{:08x} address=0x{:x}", GetCurrentProcessId(), GetCurrentThreadId(), env!("CARGO_PKG_VERSION"), record.ExceptionCode.0 as u32, record.ExceptionAddress as usize);
            let mut module = HMODULE::default();
            if GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, PCWSTR(record.ExceptionAddress.cast()), &mut module).is_ok() {
                let mut name = [0u16; 512];
                let len = GetModuleFileNameW(Some(module), &mut name) as usize;
                if len > 0 && len < name.len() {
                    let start = name[..len].iter().rposition(|c| *c == b'\\' as u16 || *c == b'/' as u16).map_or(0, |i| i + 1);
                    let _ = write!(output, " module=");
                    for c in char::decode_utf16(name[start..len].iter().copied()) {
                        let _ = write!(output, "{}", c.unwrap_or('?'));
                    }
                    let _ = write!(output, " offset=0x{:x}", (record.ExceptionAddress as usize).saturating_sub(module.0 as usize));
                }
            }
            let _ = writeln!(output);
            let _ = WriteFile(HANDLE(handle), Some(&output.bytes[..output.len]), None, None);
            let _ = FlushFileBuffers(HANDLE(handle));
        }
        // Preserve an earlier handler and the operating system's crash reporting.
        let previous = PREVIOUS.load(Ordering::Acquire);
        if previous.is_null() { 0 } else {
            let previous: unsafe extern "system" fn(*const EXCEPTION_POINTERS) -> i32 = std::mem::transmute(previous);
            previous(info)
        }
    }
    pub fn install(directory: &Path) {
        if !FILE.load(Ordering::Acquire).is_null() { return; }
        let path = directory.join("service-native-exit.log");
        if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > 256 * 1024) {
            let _ = std::fs::rename(&path, directory.join("service-native-exit.previous.log"));
        }
        if let Ok(file) = OpenOptions::new().append(true).create(true).open(path) {
            FILE.store(file.into_raw_handle(), Ordering::Release);
            let previous = unsafe { SetUnhandledExceptionFilter(Some(record)) };
            PREVIOUS.store(previous.map_or(std::ptr::null_mut(), |handler| handler as *mut std::ffi::c_void), Ordering::Release);
        }
    }
}

pub fn install(directory: &std::path::Path) {
    #[cfg(target_os = "windows")]
    windows::install(directory);
    #[cfg(target_os = "linux")]
    let _ = directory;
}
