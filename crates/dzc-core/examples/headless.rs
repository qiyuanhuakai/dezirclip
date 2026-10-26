//! A stable Core consumer: no connection handles, native adapter, or application runtime.
use dzc_core::api::{ClipboardEntry, CoreErrorCode, HistoryQuery, HistoryService};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let history = HistoryService::open(std::path::Path::new(":memory:"))?;
    use dzc_core::capture::*;
    use std::collections::VecDeque;
    let session=dzc_core::api::SessionHistoryStore::new(VecDeque::new());
    let context=CaptureContext {source:CaptureSource {app_name:"headless-example".into(),process_path:None},timestamp:2,
        settings:CaptureSettings {persistent:true,deduplicate:true,..Default::default()},session:&session,paused:false,sequential_echo:None};
    let directory=std::env::temp_dir();
    let store=history.capture_store(&directory);
    let captured=CaptureEngine::capture_frame(&context,CaptureFrame {data:CaptureData::Text("typed capture".into()),metadata:CaptureReadMetadata {privacy_ticket:store.capture_ticket(chrono::Utc::now().timestamp_millis()).expect("shared privacy state must be available"),..Default::default()}},&store,&mut ());
    let CaptureOutcome::Stored(captured)=captured else {panic!("capture should use the stable history store");};
    assert!(captured.persistent);assert_eq!(history.get_entry_by_id(captured.entry.id)?.unwrap().content,"typed capture");
    assert!(captured.privacy.evidence.iter().any(|evidence|evidence.reason==dzc_core::privacy::PrivacyReason::PrivacyHintsUnavailable));
    let mut entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
        "id":0,"content_type":"text","content":"Core 中文🙂\r\n","preview":"Core 中文🙂",
        "source_app":"headless-example","timestamp":1,"is_pinned":false
    }))?;
    let id = history.save(&entry, None)?;
    let rows = history.query(&HistoryQuery {
        id: Some(id),
        limit: 1,
        ..Default::default()
    })?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content, entry.content);
    entry.id = id;
    entry.tags = vec!["sensitive".to_owned()];
    assert_eq!(
        history.save(&entry, None).unwrap_err().code,
        CoreErrorCode::KeyUnavailable
    );
    assert_eq!(
        history.get_entry_by_id(id)?.unwrap().tags,
        Vec::<String>::new()
    );
    println!(
        "{}",
        serde_json::json!({"history_roundtrip":true,"unconfigured_provider":"KeyUnavailable"})
    );
    Ok(())
}
