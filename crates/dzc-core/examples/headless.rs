//! A real Core consumer: no native adapter, graphical session, or application runtime.
use dzc_core::domain::models::ClipboardEntry;
use dzc_core::infrastructure::encryption::{encrypt_sensitive, EncryptionError};
use dzc_core::infrastructure::repository::{
    cli_query::CliQuery,
    clipboard_repo::{ClipboardRepository, SqliteClipboardRepository},
};
use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = dzc_core::database::init_db(":memory:")?;
    let repository = SqliteClipboardRepository::new(Arc::new(Mutex::new(connection)));
    let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
        "id": 0, "content_type": "text", "content": "Core 中文🙂\r\n",
        "preview": "Core 中文🙂", "source_app": "headless-example",
        "timestamp": 1, "is_pinned": false
    }))?;
    let id = repository.save(&entry, None)?;
    let rows = repository.query_cli(&CliQuery {
        id: Some(id),
        limit: 1,
        ..Default::default()
    })?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content, entry.content);
    assert_eq!(
        encrypt_sensitive("synthetic private probe").unwrap_err(),
        EncryptionError::KeyUnavailable
    );
    println!(
        "{}",
        serde_json::json!({"history_roundtrip": true, "unconfigured_provider": "KeyUnavailable"})
    );
    Ok(())
}
