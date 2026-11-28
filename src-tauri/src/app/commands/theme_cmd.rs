use crate::app_state::SettingsState;
use dzc_core::api::HistoryService;
use crate::error::{AppError, AppResult};
use dzc_core::domain::theme::{valid_theme_id, ThemePackage, MAX_THEME_BYTES};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State, WebviewWindow};

#[derive(Default)]
pub struct ThemePublicationState(Mutex<()>);

impl ThemePublicationState {
    // Serialize this GUI's catalog commits and their notifications. Native
    // appearance changes and other Host clients remain outside this section.
    fn commit_and_publish<T>(
        &self,
        commit: impl FnOnce() -> AppResult<T>,
        publish: impl FnOnce(&T),
    ) -> AppResult<T> {
        let _guard = self.0.lock().map_err(theme_error)?;
        let outcome = commit()?;
        publish(&outcome);
        Ok(outcome)
    }
}

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
pub fn get_custom_themes(state: State<'_, HistoryService>) -> AppResult<Vec<ThemePackage>> {
    state.custom_themes().map_err(theme_error)
}

fn save_theme(
    app: &AppHandle,
    state: &HistoryService,
    publication: &ThemePublicationState,
    theme: ThemePackage,
) -> AppResult<ThemePackage> {
    let outcome = publication.commit_and_publish(
        || state.import_theme(theme).map_err(theme_error),
        |outcome| emit_library(app, &outcome.themes),
    )?;
    Ok(outcome.theme)
}

#[tauri::command]
pub async fn import_custom_theme(
    app: AppHandle,
    state: State<'_, HistoryService>,
    publication: State<'_, ThemePublicationState>,
    path: String,
) -> AppResult<ThemePackage> {
    let theme = tauri::async_runtime::spawn_blocking(move || {
        let bytes = dzc_core::filesystem::read_bounded_regular(
            std::path::Path::new(&path),
            MAX_THEME_BYTES,
        )
        .map_err(theme_error)?;
        ThemePackage::parse(&bytes).map_err(theme_error)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    save_theme(&app, &state, &publication, theme)
}

#[tauri::command]
pub fn delete_custom_theme(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, HistoryService>,
    settings: State<'_, SettingsState>,
    publication: State<'_, ThemePublicationState>,
    id: String,
) -> AppResult<()> {
    if !valid_theme_id(&id) {
        return Err(theme_error("invalid custom theme id"));
    }
    let outcome = publication.commit_and_publish(
        || state.remove_theme_with_outcome(&id).map_err(theme_error),
        |outcome| emit_library(&app, &outcome.themes),
    )?;
    // The publication guard is released before any native window work.
    if outcome.active_reset {
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
    state: State<'_, HistoryService>,
    id: String,
    path: String,
) -> AppResult<()> {
    if !valid_theme_id(&id) {
        return Err(theme_error("invalid custom theme id"));
    }
    let history = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        history.export_theme_file(&id, std::path::Path::new(&path)).map_err(theme_error)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

pub fn effective_base_theme(state: &HistoryService, theme: &str) -> String {
    state.effective_base_theme(theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dzc_core::api::HistoryService;
    use std::path::Path;
    use std::sync::{mpsc, TryLockError};
    use std::time::Duration;

    fn sample(id: &str) -> ThemePackage {
        ThemePackage {
            format: "dezirclip-theme".into(),
            version: 1,
            id: id.into(),
            name: id.into(),
            author: None,
            description: None,
            base: "mica".into(),
            css: ":root.light-mode{--dc-accent:#123456}".into(),
        }
    }

    fn ids(themes: &[ThemePackage]) -> Vec<String> {
        themes.iter().map(|theme| theme.id.clone()).collect()
    }

    #[test]
    fn theme_publication_preserves_commit_order_while_a_publisher_is_paused() {
        let history = HistoryService::open(Path::new(":memory:")).unwrap();
        let publication = ThemePublicationState::default();
        let events = Mutex::new(Vec::new());
        let (committed_tx, committed_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let timeout = Duration::from_secs(5);

        std::thread::scope(|scope| {
            let history = &history;
            let publication = &publication;
            let events = &events;
            scope.spawn(move || {
                publication
                    .commit_and_publish(
                        || history.import_theme(sample("first")).map_err(theme_error),
                        |outcome| {
                            committed_tx.send(()).unwrap();
                            resume_rx.recv_timeout(timeout).unwrap();
                            events.lock().unwrap().push(ids(&outcome.themes));
                        },
                    )
                    .unwrap();
            });
            committed_rx.recv_timeout(timeout).unwrap();
            // The database is available, but a second GUI commit must still
            // wait until the first committed snapshot has been published.
            assert_eq!(ids(&history.custom_themes().unwrap()), vec!["first"]);
            assert!(matches!(publication.0.try_lock(), Err(TryLockError::WouldBlock)));
            scope.spawn(move || {
                attempt_tx.send(()).unwrap();
                publication
                    .commit_and_publish(
                        || history.remove_theme_with_outcome("first").map_err(theme_error),
                        |outcome| events.lock().unwrap().push(ids(&outcome.themes)),
                    )
                    .unwrap();
            });
            attempt_rx.recv_timeout(timeout).unwrap();
            resume_tx.send(()).unwrap();
        });

        assert_eq!(*events.lock().unwrap(), vec![vec!["first".to_string()], Vec::new()]);
        assert!(history.custom_themes().unwrap().is_empty());
        assert!(publication.0.try_lock().is_ok());
    }

    #[test]
    fn theme_publication_does_not_emit_failed_mutations_or_change_error_kind() {
        let history = HistoryService::open(Path::new(":memory:")).unwrap();
        history.import_theme(sample("first")).unwrap();
        let publication = ThemePublicationState::default();
        let events = Mutex::new(Vec::new());

        let error = publication
            .commit_and_publish(
                || history.import_theme(sample("first")).map_err(theme_error),
                |outcome| events.lock().unwrap().push(ids(&outcome.themes)),
            )
            .unwrap_err();
        assert!(matches!(&error, AppError::Validation(_)));
        assert_eq!(
            error.to_string(),
            "验证错误: a theme with this id already exists; edit its id before importing",
        );
        assert!(events.lock().unwrap().is_empty());
        assert_eq!(ids(&history.custom_themes().unwrap()), vec!["first"]);

        publication
            .commit_and_publish(
                || history.import_theme(sample("second")).map_err(theme_error),
                |outcome| events.lock().unwrap().push(ids(&outcome.themes)),
            )
            .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec![vec!["first".to_string(), "second".to_string()]],
        );
        assert!(publication.0.try_lock().is_ok());
    }
}
