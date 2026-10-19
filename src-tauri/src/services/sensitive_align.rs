use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::database::{DbState, SENSITIVE_TAGS};
use crate::infrastructure::repository::settings_repo::SettingsRepository;

pub fn spawn_sensitive_alignment(app_handle: AppHandle) {
    thread::spawn(move || run_alignment(app_handle));
}

fn needs_alignment(content: &str, preview: &str, html: Option<&str>, ocr: Option<&str>) -> bool {
    [Some(content), Some(preview), html, ocr]
        .into_iter()
        .flatten()
        .any(|value| !crate::infrastructure::encryption::has_ciphertext_envelope(value))
}

fn run_alignment(app_handle: AppHandle) {
    let db_state = app_handle.state::<DbState>();
    let done = db_state
        .settings_repo
        .get("db.sensitive_alignment_done")
        .unwrap_or(None)
        .unwrap_or_else(|| "false".to_string());
    if done == "true" {
        return;
    }

    let sensitive_tags_sql = {
        let parts: Vec<String> = SENSITIVE_TAGS
            .iter()
            .map(|t| format!("'{}'", t.replace('\'', "''")))
            .collect();
        format!("({})", parts.join(","))
    };

    let mut cursor_ts = i64::MAX;
    let mut cursor_id = i64::MAX;
    let batch_size = 200;
    let mut complete = true;

    loop {
        let conn_guard = match db_state.conn.lock() {
            Ok(c) => c,
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        };

        let sql = format!(
            "SELECT ch.id, ch.timestamp, ch.content, ch.preview, ch.html_content,
                    EXISTS (
                        SELECT 1 FROM entry_tags se
                        WHERE se.entry_id = ch.id
                          AND se.tag COLLATE NOCASE IN {}
                    ) AS is_sensitive, ch.ocr_text, ch.tags
             FROM clipboard_history ch
             WHERE (ch.timestamp < ?1) OR (ch.timestamp = ?1 AND ch.id < ?2)
             ORDER BY ch.timestamp DESC, ch.id DESC
             LIMIT ?3",
            sensitive_tags_sql
        );

        let mut batch: Vec<(
            i64,
            i64,
            String,
            String,
            Option<String>,
            bool,
            Option<String>,
            String,
        )> = Vec::new();
        {
            let mut stmt = match conn_guard.prepare(&sql) {
                Ok(s) => s,
                Err(_) => {
                    complete = false;
                    break;
                }
            };

            let rows = match stmt.query_map([cursor_ts, cursor_id, batch_size], |row| {
                let id: i64 = row.get(0)?;
                let ts: i64 = row.get(1)?;
                let content: String = row.get(2)?;
                let preview: String = row.get(3)?;
                let html: Option<String> = row.get(4)?;
                let is_sensitive: i32 = row.get(5)?;
                Ok((
                    id,
                    ts,
                    content,
                    preview,
                    html,
                    is_sensitive == 1,
                    row.get(6)?,
                    row.get(7)?,
                ))
            }) {
                Ok(r) => r,
                Err(_) => {
                    complete = false;
                    break;
                }
            };

            for row in rows {
                match row {
                    Ok(item) => batch.push(item),
                    Err(_) => complete = false,
                }
            }
        }

        if batch.is_empty() {
            break;
        }

        for (id, _ts, content, preview, html, linked_sensitive, ocr, raw_tags) in batch.iter() {
            let Ok(tags) = serde_json::from_str::<Vec<String>>(raw_tags) else {
                complete = false;
                continue;
            };
            if (*linked_sensitive || crate::database::has_sensitive_tag(&tags))
                && needs_alignment(content, preview, html.as_deref(), ocr.as_deref())
                && db_state
                    .repo
                    .encrypt_entry_with_conn(&conn_guard, *id)
                    .is_err()
            {
                complete = false;
            }
        }

        if let Some((id, ts, _, _, _, _, _, _)) = batch.last() {
            cursor_ts = *ts;
            cursor_id = *id;
        } else {
            break;
        }

        drop(conn_guard);
        thread::sleep(Duration::from_millis(50));
    }

    if complete {
        let _ = db_state
            .settings_repo
            .set("db.sensitive_alignment_done", "true");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn legacy_ocr_and_ordinary_prefix_text_need_alignment() {
        let encrypted = format!(
            "linux:{}",
            base64::engine::general_purpose::STANDARD.encode([1u8; 40])
        );
        assert!(!needs_alignment(
            &encrypted,
            &encrypted,
            Some(&encrypted),
            Some(&encrypted)
        ));
        assert!(needs_alignment(
            &encrypted,
            &encrypted,
            Some(&encrypted),
            Some("legacy private OCR")
        ));
        assert!(needs_alignment(
            "linux:ordinary prefix",
            &encrypted,
            None,
            None
        ));
    }
}
