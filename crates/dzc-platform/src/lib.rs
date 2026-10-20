//! OS adapters depend on Core's ports; Core never depends on this crate.
pub mod encryption;
pub mod capture;
pub mod local_file;
pub mod runtime_display;
pub mod native_clipboard;
pub mod runtime;
#[cfg(feature = "ipc")]
pub mod ipc;
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
