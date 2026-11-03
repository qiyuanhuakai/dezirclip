use crate::{app::ipc_host::DesktopHost, database::DbState, error::AppError};
use dzc_host::wire::{Method, OperationResult, RuntimeState};
use tauri::State;

#[tauri::command]
pub async fn open_content(
    host: State<'_, DesktopHost>,
    state: State<'_, DbState>,
    id: i64,
    content: String,
    content_type: String,
    expected_privacy_epoch: Option<u64>,
    expected_host_instance_id: Option<String>,
) -> Result<(), AppError> {
    host.check_instance(id, expected_host_instance_id.as_deref()).map_err(AppError::Validation)?;
    if id < 0 { return Err(AppError::Validation("SessionExternalOpenUnavailable".into())); }
    if id == 0 {
        let history = state.history_service();
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
    use std::sync::{Arc,Mutex};

    fn fixture()->(dzc_core::api::HistoryService,Arc<Mutex<rusqlite::Connection>>) {
        let connection=crate::database::init_db(":memory:").unwrap();
        connection.execute("INSERT INTO clipboard_history(id,content_type,content,preview,tags,source_app,timestamp,is_pinned) VALUES(1,'url','https://example.invalid/','synthetic','[]','Synthetic',1,0)",[]).unwrap();
        let connection=Arc::new(Mutex::new(connection));
        (dzc_core::api::HistoryService::from_legacy_connection(connection.clone()),connection)
    }

    fn permit(history:&dzc_core::api::HistoryService,now:i64)->dzc_core::api::PrivacyReadPermit {
        history.privacy().authorize_selected_copy(1,None,dzc_core::api::SensitiveAccess::Denied,None,now).unwrap()
    }

    fn uses(connection:&Arc<Mutex<rusqlite::Connection>>)->i32 {
        connection.lock().unwrap().query_row("SELECT use_count FROM clipboard_history WHERE id=1",[],|row|row.get(0)).unwrap()
    }

    #[test]
    fn external_open_uses_only_handler_preferences_and_code_falls_back_to_text() {
        let (history, connection) = fixture();
        connection.lock().unwrap().execute_batch("INSERT OR REPLACE INTO settings(key,value) VALUES
            ('app.text','text-editor'),('app.code',''),('app.url','browser'),('app.custom_background','user-image')").unwrap();
        assert_eq!(history.default_application("code").unwrap().as_deref(), Some("text-editor"));
        assert_eq!(history.default_application("link").unwrap().as_deref(), Some("browser"));
        assert_eq!(history.default_application("custom_background").unwrap(), None);
        connection.lock().unwrap().execute("UPDATE settings SET value='code-editor' WHERE key='app.code'", []).unwrap();
        assert_eq!(history.default_application("code").unwrap().as_deref(), Some("code-editor"));
    }

    #[test]
    fn paused_recording_allows_read_only_open_without_an_edit_write_ticket() {
        let (history,_)=fixture();
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
        let (history,connection)=fixture();
        let permit=permit(&history,10);
        assert!(complete_external_open(&history,1,&permit,Err(WireErrorCode::Unavailable),10).is_err());
        assert_eq!(uses(&connection),0);
        complete_external_open(&history,0,&permit,Ok(()),10).unwrap();
        assert_eq!(uses(&connection),0);
        complete_external_open(&history,1,&permit,Ok(()),10).unwrap();
        assert_eq!(uses(&connection),1);
        history.privacy().toggle(11).unwrap();
        assert!(complete_external_open(&history,1,&permit,Ok(()),12).is_err());
        assert_eq!(uses(&connection),1);
    }
}
