use std::thread;
use tauri::{AppHandle, Manager};

use dzc_core::api::HistoryService;

pub fn spawn_sensitive_alignment(app_handle: AppHandle) {
    let history = app_handle.state::<HistoryService>().inner().clone();
    thread::spawn(move || {
        if let Err(error) = history.align_legacy_sensitive_history() {
            eprintln!("Legacy sensitive history alignment is incomplete: {error}");
        }
    });
}
