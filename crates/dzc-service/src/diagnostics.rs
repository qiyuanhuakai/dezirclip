//! Bounded lifecycle evidence; never persist request data or panic payloads.
use std::{fs::{File, OpenOptions}, io::Write, path::Path, sync::{Arc, Mutex}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Clone)]
pub(crate) struct Diagnostics(Arc<Mutex<Option<File>>>);
impl Diagnostics {
    pub(crate) fn open(directory: &Path) -> Self {
        let path = directory.join("service-lifecycle.log");
        if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 256 * 1024) {
            let _ = std::fs::rename(&path, directory.join("service-lifecycle.previous.log"));
        }
        let value = Self(Arc::new(Mutex::new(OpenOptions::new().create(true).append(true).open(path).ok())));
        value.record(&format!("start version={}", env!("CARGO_PKG_VERSION")));
        value
    }
    pub(crate) fn record(&self, event: &str) {
        if let Ok(mut guard) = self.0.try_lock() {
            if let Some(file) = guard.as_mut() {
                let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
                let _ = writeln!(file, "{now} pid={} {event}", std::process::id());
                let _ = file.flush();
            }
        }
    }
    pub(crate) fn install_panic_hook(&self) {
        let diagnostics = self.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(location) = info.location() {
                diagnostics.record(&format!("panic source={}:{}:{}", location.file(), location.line(), location.column()));
            } else {
                diagnostics.record("panic source=unknown");
            }
            previous(info);
        }));
    }
}
