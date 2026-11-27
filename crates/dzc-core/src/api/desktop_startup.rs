use super::{CoreResult, HistoryService};

pub struct DesktopStartupPreferences {
    pub theme: String,
    pub persistent: bool,
    pub capture_files: bool,
    pub capture_rich_text: bool,
    pub deduplicate: bool,
    pub silent_start: bool,
    pub delete_after_paste: bool,
    pub privacy_protection: bool,
    pub privacy_kinds: String,
    pub privacy_custom: String,
    pub excluded_source_apps: String,
    pub sequential_mode: bool,
    pub sequential_hotkey: String,
    pub rich_paste_hotkey: String,
    pub search_hotkey: String,
    pub screenshot_enabled: bool,
    pub screenshot_hotkey: String,
    pub screenshot_full_hotkey: String,
    pub quick_paste_enabled: bool,
    pub quick_paste_hotkey: String,
    pub ocr_enabled: bool,
    pub sound_enabled: bool,
    pub hide_tray_icon: bool,
    pub edge_docking: bool,
    pub follow_mouse: bool,
    pub window_pinned: bool,
    pub window_size: Option<(u32, u32)>,
    pub main_hotkey: String,
    pub arrow_key_selection: bool,
    pub idle_destroy_enabled: bool,
    pub idle_destroy_seconds: Option<u64>,
}

impl HistoryService {
    pub fn desktop_startup_preferences(&self) -> DesktopStartupPreferences {
        self.repository.desktop_startup_preferences()
    }

    pub fn saved_window_size(&self) -> CoreResult<Option<(u32, u32)>> {
        self.repository.saved_window_size()
    }

    pub fn window_pinned(&self) -> CoreResult<bool> {
        self.repository.window_pinned()
    }

    pub fn reset_unelevated_game_mode(&self, is_elevated: impl FnOnce() -> bool) -> bool {
        self.repository.reset_unelevated_game_mode(is_elevated)
    }
}

#[cfg(test)]
mod tests {
    use crate::database::DbState;
    use crate::infrastructure::repository::settings_repo::SettingsRepository;

    #[test]
    fn desktop_startup_keeps_gui_initialization_and_saved_preferences() {
        let path =
            std::env::temp_dir().join(format!("dezirclip-startup-{}.db", uuid::Uuid::new_v4()));
        {
            let database = DbState::open_desktop(&path).unwrap();
            let history = database.history_service();
            let initial = history.desktop_startup_preferences();
            assert_eq!(initial.theme, crate::domain::theme::DEFAULT_BUILTIN_THEME);
            assert!(!initial.persistent);
            assert_eq!(initial.window_size, None);
            let table = database.conn.lock().unwrap();
            assert_eq!(
                table
                    .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                    .unwrap(),
                "wal"
            );
            assert_eq!(
                table
                    .pragma_query_value(None, "synchronous", |row| row.get::<_, u8>(0))
                    .unwrap(),
                1
            );
            table.execute_batch("DELETE FROM settings WHERE key='app.theme';
                INSERT OR REPLACE INTO settings VALUES ('app.capture_files', 'invalid'), ('app.search_hotkey', ''),
                ('app.paste_method', 'game_mode'), ('app.window_pinned', 'true'),
                ('app.window_width', '800'), ('app.window_height', '480'), ('app.idle_destroy_seconds', '-2');").unwrap();
            drop(table);
            let saved = history.desktop_startup_preferences();
            assert_eq!(saved.theme, "retro");
            assert!(!saved.capture_files);
            assert_eq!(saved.search_hotkey, "");
            assert_eq!(saved.window_size, Some((800, 480)));
            assert_eq!(saved.idle_destroy_seconds, None);
            assert_eq!(history.saved_window_size().unwrap(), saved.window_size);
            assert!(history.window_pinned().unwrap());
            assert!(!history.reset_unelevated_game_mode(|| true));
            assert!(history.reset_unelevated_game_mode(|| false));
            assert_eq!(
                database
                    .settings_repo
                    .get("app.paste_method")
                    .unwrap()
                    .as_deref(),
                Some("shift_insert")
            );
            assert!(!history.reset_unelevated_game_mode(|| panic!("unnecessary elevation check")));
        }
        std::fs::remove_file(path).unwrap();
    }
}
