use crate::{Host, HostActions, HostOptions};
use dzc_core::api::HistoryService;
use dzc_protocol::wire::{HostMode, WindowsPasteTarget, WireErrorCode};
use serde_json::Value;
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex}};

#[derive(Default)]
pub struct Actions { copied: Mutex<Vec<String>> }
impl Actions {
    pub fn copied(&self) -> Vec<String> { self.copied.lock().unwrap().clone() }
}
impl HostActions for Actions {
    fn platform_capabilities(&self) -> Value { serde_json::json!({"synthetic":true}) }
    fn can_copy(&self) -> bool { true }
    fn copy(&self, payload: dzc_core::ports::ClipboardPayload) -> Result<(), WireErrorCode> {
        if let dzc_core::ports::ClipboardPayload::Text(text) = payload { self.copied.lock().unwrap().push(text); Ok(()) } else { Err(WireErrorCode::Unavailable) }
    }
    fn can_paste(&self) -> bool { false }
    fn paste(&self) -> Result<(), WireErrorCode> { Err(WireErrorCode::Unavailable) }
    fn stop_helpers(&self) -> Result<(), WireErrorCode> { Ok(()) }
    fn revoke_private_exports(&self) -> Result<(), WireErrorCode> { Ok(()) }
}
struct SyntheticKeys;
fn sealed_fixture_values() -> &'static Mutex<std::collections::HashMap<String, String>> {
    static VALUES: std::sync::OnceLock<Mutex<std::collections::HashMap<String, String>>> = std::sync::OnceLock::new();
    VALUES.get_or_init(Mutex::default)
}
impl dzc_core::crypto::KeyProvider for SyntheticKeys {
    fn seal_text(&self, plain: &str) -> Result<String, dzc_core::crypto::EncryptionError> {
        // A deterministic process-local fixture uses the actual Core cipher;
        // this registry is not a platform key provider or a crypto claim.
        let sealed = dzc_core::crypto::encrypt_with_key(plain, || Ok([37;32]), |nonce| { nonce.fill(19); Ok(()) })?;
        sealed_fixture_values().lock().unwrap().insert(sealed.clone(), plain.into());
        Ok(sealed)
    }
    fn open_text(&self, cipher: &str) -> Option<String> {
        sealed_fixture_values().lock().unwrap().get(cipher).cloned()
    }
}
pub fn windows_target() -> WindowsPasteTarget {
    WindowsPasteTarget{hwnd:"10".into(),root_hwnd:"10".into(),pid:20,thread_id:30,creation_time_100ns:"40".into()}
}
pub fn synthetic_host() -> Arc<Host> {
    static KEYS: SyntheticKeys = SyntheticKeys;
    let _ = dzc_core::crypto::install_key_provider(&KEYS);
    Host::new(HostOptions { mode: HostMode::Service, history: HistoryService::open(Path::new(":memory:")).unwrap(),
        directory: std::env::temp_dir().join(format!("dzc-host-{}", uuid::Uuid::new_v4())), actions: Arc::new(Actions::default()) }).unwrap()
}

pub fn socket_directory() -> PathBuf {
    #[cfg(target_os = "macos")]
    let root = PathBuf::from("/private/tmp");
    #[cfg(not(target_os = "macos"))]
    let root = std::env::temp_dir();
    let dir = root.join(format!("dzc-{}", uuid::Uuid::new_v4().simple()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    }
    #[cfg(windows)]
    std::fs::create_dir(&dir).unwrap();
    dir
}
