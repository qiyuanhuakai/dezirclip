use std::thread;
use tauri::{AppHandle, Manager};

use crate::database::DbState;

pub fn spawn_sensitive_alignment(app_handle: AppHandle) {
    let history = app_handle.state::<DbState>().history_service();
    thread::spawn(move || {
        if let Err(error) = history.align_legacy_sensitive_history() {
            eprintln!("Legacy sensitive history alignment is incomplete: {error}");
        }
    });
}
