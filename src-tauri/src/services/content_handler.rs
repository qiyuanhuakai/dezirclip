use crate::{app::ipc_host::DesktopHost, error::AppError};
use dzc_core::api::HistoryService;
use dzc_host::wire::{Method, OperationResult, RuntimeState};
use tauri::State;

#[tauri::command]
pub async fn open_content(
    host: State<'_, DesktopHost>,
    state: State<'_, HistoryService>,
    id: i64,
    content: String,
    content_type: String,
    expected_privacy_epoch: Option<u64>,
    expected_host_instance_id: Option<String>,
) -> Result<(), AppError> {
    host.check_instance(id, expected_host_instance_id.as_deref()).map_err(AppError::Validation)?;
    if id < 0 { return Err(AppError::Validation("SessionExternalOpenUnavailable".into())); }
    if id == 0 {
        let history = state.inner().clone();
        return tokio::task::spawn_blocking(move || {
            dzc_host::external::open_generated(&history, &content, &content_type, expected_privacy_epoch)
                .map_err(|code| AppError::Validation(format!("{code:?}")))
        }).await.map_err(|error| AppError::Internal(error.to_string()))?;
    }
    let current: RuntimeState = host.call(Method::Health, serde_json::json!({}), "RuntimeState").await.map_err(AppError::Validation)?;
    let _: OperationResult = host.blocking_call(Method::DesktopHistoryOpen, serde_json::json!({
        "service_instance_id": expected_host_instance_id.unwrap_or(current.service_instance_id),
        "id": id, "expected_epoch": expected_privacy_epoch.unwrap_or(current.privacy_epoch),
        "reveal": expected_privacy_epoch.is_some()
    }), "OperationResult").await.map_err(AppError::Validation)?;
    Ok(())
}

#[cfg(test)]
mod authorization_tests {
    use dzc_host::external::{complete_external_open, edit_watch_ticket};
    use dzc_host::wire::WireErrorCode;
    use dzc_core::api::{ClipboardEntry, HistoryService, ImportMode, PrivacyReadPermit, SensitiveAccess};

    fn fixture() -> HistoryService {
        let history = HistoryService::open_desktop(std::path::Path::new(":memory:")).unwrap();
        let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
            "id": 1, "content_type": "url", "content": "https://example.invalid/",
            "preview": "synthetic", "source_app": "Synthetic", "timestamp": 1, "is_pinned": false
        })).unwrap();
        history.restore(&[entry], ImportMode::Merge).unwrap();
        history
    }

    fn permit(history: &HistoryService, now: i64) -> PrivacyReadPermit {
        history.privacy().authorize_selected_copy(1, None, SensitiveAccess::Denied, None, now).unwrap()
    }

    fn uses(history: &HistoryService) -> i32 {
        history.get_entry_by_id(1).unwrap().unwrap().use_count
    }

    #[test]
    fn external_open_uses_only_handler_preferences_and_code_falls_back_to_text() {
        let history = fixture();
        let epoch = history.runtime_snapshot(10).unwrap().privacy_epoch;
        history.set_default_application("app.text", "text-editor", epoch).unwrap();
        history.set_default_application("app.code", "", epoch).unwrap();
        history.set_default_application("app.url", "browser", epoch).unwrap();
        history.set_desktop_setting("app.custom_background", "user-image").unwrap();
        assert_eq!(history.default_application("code").unwrap().as_deref(), Some("text-editor"));
        assert_eq!(history.default_application("link").unwrap().as_deref(), Some("browser"));
        assert_eq!(history.default_application("custom_background").unwrap(), None);
        history.set_default_application("app.code", "code-editor", epoch).unwrap();
        assert_eq!(history.default_application("code").unwrap().as_deref(), Some("code-editor"));
    }

    #[test]
    fn paused_recording_allows_read_only_open_without_an_edit_write_ticket() {
        let history = fixture();
        history.privacy().toggle(10).unwrap();
        let permit=permit(&history,11);
        assert!(history.get_entry_authorized(1,&permit,11).unwrap().is_some());
        for kind in ["url","files","video","image","text","rich_text","code"] {
            assert!(edit_watch_ticket(&history,&permit,1,kind,11).unwrap().is_none(),"{kind}");
        }
        history.privacy().toggle(12).unwrap();
        assert_eq!(edit_watch_ticket(&history,&permit,1,"text",13).unwrap_err(),dzc_core::api::PrivacyError::PrivacyEpochChanged);
        let current=self::permit(&history,13);
        assert_eq!(edit_watch_ticket(&history,&current,1,"text",13).unwrap().unwrap().epoch(),current.epoch());
        assert!(edit_watch_ticket(&history,&current,1,"files",13).unwrap().is_none());
    }

    #[test]
    fn only_successful_current_persistent_opens_increment_use_count_once() {
        let history = fixture();
        let permit=permit(&history,10);
        assert!(complete_external_open(&history,1,&permit,Err(WireErrorCode::Unavailable),10).is_err());
        assert_eq!(uses(&history),0);
        complete_external_open(&history,0,&permit,Ok(()),10).unwrap();
        assert_eq!(uses(&history),0);
        complete_external_open(&history,1,&permit,Ok(()),10).unwrap();
        assert_eq!(uses(&history),1);
        history.privacy().toggle(11).unwrap();
        assert!(complete_external_open(&history,1,&permit,Ok(()),12).is_err());
        assert_eq!(uses(&history),1);
    }
}
