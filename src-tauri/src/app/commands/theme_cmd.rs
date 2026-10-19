use crate::app_state::SettingsState;
use crate::database::DbState;
use crate::domain::theme::{valid_theme_id, ThemePackage, MAX_THEME_BYTES};
use crate::error::{AppError, AppResult};
use crate::infrastructure::repository::custom_themes;
use tauri::{AppHandle, Emitter, State, WebviewWindow};

fn theme_error(error: impl ToString) -> AppError {
    AppError::Validation(error.to_string())
}

fn emit_library(app: &AppHandle, themes: &[ThemePackage]) {
    let _ = app.emit(
        "custom-themes-changed",
        serde_json::json!({"themes":themes}),
    );
}

#[tauri::command]
pub fn get_custom_themes(state: State<'_, DbState>) -> AppResult<Vec<ThemePackage>> {
    let conn = state.conn.lock().map_err(theme_error)?;
    custom_themes::load(&conn).map_err(theme_error)
}

fn save_theme(app: &AppHandle, state: &DbState, theme: ThemePackage) -> AppResult<ThemePackage> {
    let id = theme.id.clone();
    let themes = {
        let mut conn = state.conn.lock().map_err(theme_error)?;
        let themes = custom_themes::upsert(&mut conn, theme, false).map_err(theme_error)?;
        emit_library(app, &themes);
        themes
    };
    let saved = themes
        .iter()
        .find(|theme| theme.id == id)
        .cloned()
        .ok_or_else(|| theme_error("saved theme not found"))?;
    Ok(saved)
}

#[tauri::command]
pub async fn import_custom_theme(
    app: AppHandle,
    state: State<'_, DbState>,
    path: String,
) -> AppResult<ThemePackage> {
    let theme = tauri::async_runtime::spawn_blocking(move || {
        let bytes = crate::infrastructure::local_file::read_bounded_regular(
            std::path::Path::new(&path),
            MAX_THEME_BYTES,
        )
        .map_err(theme_error)?;
        ThemePackage::parse(&bytes).map_err(theme_error)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    save_theme(&app, &state, theme)
}

#[tauri::command]
pub fn delete_custom_theme(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, DbState>,
    settings: State<'_, SettingsState>,
    id: String,
) -> AppResult<()> {
    if !valid_theme_id(&id) {
        return Err(theme_error("invalid custom theme id"));
    }
    let (_, active) = {
        let mut conn = state.conn.lock().map_err(theme_error)?;
        let removed = custom_themes::remove(&mut conn, &id).map_err(theme_error)?;
        emit_library(&app, &removed.0);
        removed
    };
    if active {
        super::ui_cmd::set_theme(
            app.clone(),
            window,
            settings,
            state,
            "mica".into(),
            None,
            None,
        )?;
        let _ = app.emit("settings-changed", ());
    }
    Ok(())
}

#[tauri::command]
pub async fn export_custom_theme(
    state: State<'_, DbState>,
    id: String,
    path: String,
) -> AppResult<()> {
    if !valid_theme_id(&id) {
        return Err(theme_error("invalid custom theme id"));
    }
    let theme = {
        let conn = state.conn.lock().map_err(theme_error)?;
        custom_themes::load(&conn)
            .map_err(theme_error)?
            .into_iter()
            .find(|theme| theme.id == id)
            .ok_or_else(|| theme_error("custom theme not found"))?
    };
    let repository =
        crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository::new(
            state.conn.clone(),
        );
    tauri::async_runtime::spawn_blocking(move || {
        let target = std::path::Path::new(&path);
        repository
            .ensure_export_destination(target)
            .map_err(theme_error)?;
        crate::infrastructure::local_file::ensure_regular_destination(target)
            .map_err(theme_error)?;
        let bytes = theme.export_bytes().map_err(theme_error)?;
        crate::infrastructure::attachments::write_backup(target, &bytes).map_err(theme_error)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

pub fn effective_base_theme(state: &DbState, theme: &str) -> String {
    let Some(id) = theme.strip_prefix("custom:") else {
        return theme.to_string();
    };
    state
        .conn
        .lock()
        .ok()
        .and_then(|conn| custom_themes::load(&conn).ok())
        .and_then(|themes| themes.into_iter().find(|item| item.id == id))
        .map(|item| item.base)
        .unwrap_or_else(|| "mica".into())
}
