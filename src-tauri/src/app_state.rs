use dzc_core::api::DesktopStartupPreferences;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Default)]
pub struct SettingsState {
    pub deduplicate: AtomicBool,
    pub persistent: AtomicBool,
    pub theme: Mutex<String>,
    pub capture_files: AtomicBool,
    pub capture_rich_text: AtomicBool,
    pub silent_start: AtomicBool,
    pub delete_after_paste: AtomicBool,
    pub privacy_protection: AtomicBool,
    pub privacy_protection_kinds: Mutex<Vec<String>>,
    pub privacy_protection_custom_rules: Mutex<Vec<String>>,
    pub excluded_source_apps: Mutex<Vec<String>>,
    pub sequential_mode: AtomicBool,
    pub sequential_paste_hotkey: Mutex<String>,
    pub rich_paste_hotkey: Mutex<String>,
    pub search_hotkey: Mutex<String>,
    pub screenshot_enabled: AtomicBool,
    pub screenshot_hotkey: Mutex<String>,
    pub screenshot_full_hotkey: Mutex<String>,
    pub quick_paste_enabled: AtomicBool,
    pub quick_paste_hotkey: Mutex<String>,
    pub ocr_enabled: AtomicBool,
    pub sound_enabled: AtomicBool,
    pub hide_tray_icon: AtomicBool,
    pub edge_docking: AtomicBool,
    pub follow_mouse: AtomicBool,
    pub arrow_key_selection: AtomicBool,
    pub main_hotkey: Mutex<String>,
    pub monitors: Mutex<Vec<tauri::Monitor>>,
    pub idle_destroy_enabled: AtomicBool,
    pub idle_destroy_seconds: AtomicU64,
}

impl SettingsState {
    pub(crate) fn apply_preferences(&self, preferences: &DesktopStartupPreferences) {
        for (setting, value) in [
            (&self.deduplicate, preferences.deduplicate),
            (&self.persistent, preferences.persistent),
            (&self.capture_files, preferences.capture_files),
            (&self.capture_rich_text, preferences.capture_rich_text),
            (&self.silent_start, preferences.silent_start),
            (&self.delete_after_paste, preferences.delete_after_paste),
            (&self.privacy_protection, preferences.privacy_protection),
            (&self.sequential_mode, preferences.sequential_mode),
            (&self.screenshot_enabled, preferences.screenshot_enabled),
            (&self.quick_paste_enabled, preferences.quick_paste_enabled),
            (&self.ocr_enabled, preferences.ocr_enabled),
            (&self.sound_enabled, preferences.sound_enabled),
            (&self.hide_tray_icon, preferences.hide_tray_icon),
            (&self.edge_docking, preferences.edge_docking),
            (&self.follow_mouse, preferences.follow_mouse),
            (&self.arrow_key_selection, preferences.arrow_key_selection),
            (&self.idle_destroy_enabled, preferences.idle_destroy_enabled),
        ] {
            setting.store(value, Ordering::Relaxed);
        }
        for (setting, value) in [
            (&self.theme, &preferences.theme),
            (&self.main_hotkey, &preferences.main_hotkey),
            (&self.sequential_paste_hotkey, &preferences.sequential_hotkey),
            (&self.rich_paste_hotkey, &preferences.rich_paste_hotkey),
            (&self.search_hotkey, &preferences.search_hotkey),
            (&self.screenshot_hotkey, &preferences.screenshot_hotkey),
            (&self.screenshot_full_hotkey, &preferences.screenshot_full_hotkey),
            (&self.quick_paste_hotkey, &preferences.quick_paste_hotkey),
        ] {
            setting.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone_from(value);
        }
        *self.privacy_protection_kinds.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            preferences.privacy_kinds.split(',').map(|value| value.trim().to_owned()).collect();
        *self.privacy_protection_custom_rules.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            preferences.privacy_custom.lines().map(|value| value.trim().to_owned()).collect();
        *self.excluded_source_apps.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            crate::services::source_exclusion::parse_excluded_source_apps(&preferences.excluded_source_apps);
        self.idle_destroy_seconds.store(
            preferences.idle_destroy_seconds.map(crate::app::idle_destroyer::clamp_idle_seconds)
                .unwrap_or(crate::app::idle_destroyer::DEFAULT_IDLE_DESTROY_SECONDS),
            Ordering::Relaxed,
        );
    }
}

pub struct SessionHistory(pub std::sync::Arc<dzc_core::api::SessionHistoryStore>);

pub struct AppDataDir(pub Mutex<std::path::PathBuf>);

const SEARCH_HISTORY_MAX: usize = 20;

#[derive(Default)]
pub struct SearchHistory(pub Mutex<VecDeque<String>>);

impl SearchHistory {
    pub fn push(&self, query: String) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.0.lock() {
            guard.retain(|existing| existing != trimmed);
            guard.push_front(trimmed.to_string());
            while guard.len() > SEARCH_HISTORY_MAX {
                guard.pop_back();
            }
        }
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|guard| guard.iter().cloned().collect())
            .unwrap_or_default()
    }
}
