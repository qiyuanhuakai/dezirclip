//! OS adapters depend on Core's ports; Core never depends on this crate.
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod apps;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod desktop_system;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod desktop_tools;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod login_start;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod cli_path;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod sound;
pub mod encryption;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod capture;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod local_file;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod user_resource;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod runtime_display;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod native_clipboard;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod runtime;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod startup;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod process_memory;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod crash_diagnostics;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod video_preview;
#[cfg(target_os = "windows")]
mod export_guard;

pub fn dispatch_export_guardian() {
    #[cfg(target_os = "windows")]
    export_guard::dispatch();
}
#[cfg(any(feature = "native-fonts", all(feature = "ipc", target_os = "linux")))]
pub mod fonts;
#[cfg(feature = "ipc")]
pub mod ipc;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod shortcut;
#[cfg(all(feature = "native-ocr", any(target_os = "windows", target_os = "linux")))]
pub mod ocr;
#[cfg(target_os = "linux")]
pub mod wayland;

pub struct NativeKeyProvider;

impl dzc_core::crypto::KeyProvider for NativeKeyProvider {
    fn seal_text(
        &self,
        plain: &str,
    ) -> Result<String, dzc_core::crypto::EncryptionError> {
        encryption::encrypt_platform(plain)
    }
    fn open_text(&self, cipher: &str) -> Option<String> {
        encryption::decrypt_value(cipher)
    }
}

pub fn install_default_key_provider() {
    static PROVIDER: NativeKeyProvider = NativeKeyProvider;
    let _ = dzc_core::crypto::install_key_provider(&PROVIDER);
}

pub fn set_diagnostic_sink(sink: fn(&str)) {
    encryption::set_diagnostic_sink(sink);
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod private_files;

#[cfg(all(feature = "native-screen", any(target_os = "windows", target_os = "linux")))]
pub mod screenshot;

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod paste_input;
#[cfg(target_os = "windows")]
pub mod windows_input;
#[cfg(target_os = "windows")]
pub mod system_settings;

pub mod image_png;

#[cfg(target_os = "macos")]
pub mod darwin_clipboard;
