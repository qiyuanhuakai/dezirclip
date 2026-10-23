//! Terminal reads hold one Core-issued ordinary-data permit for the complete result.
use crate::api::{ClipboardEntry, HistoryQuery, HistoryService, PrivacyAccessIntent, PrivacyReadPermit, SensitiveAccess};

pub(crate) fn now() -> i64 { chrono::Utc::now().timestamp_millis() }

pub(crate) fn permit(history: &HistoryService) -> Result<PrivacyReadPermit, String> {
    history.privacy().authorize_read(PrivacyAccessIntent::AutomationRead, SensitiveAccess::Denied, None, now())
        .map_err(|error| error.to_string())
}

/// Preserve the CLI's unlimited-result contract while each Core request stays bounded.
/// Nothing is printed until every page and the original epoch have been checked.
pub(crate) fn query(history: &HistoryService, query: &HistoryQuery<'_>) -> Result<Vec<ClipboardEntry>, String> {
    let permit = permit(history)?;
    query_with_permit(history, query, &permit)
}

fn query_with_permit(history: &HistoryService, query: &HistoryQuery<'_>, permit: &PrivacyReadPermit) -> Result<Vec<ClipboardEntry>, String> {
    if query.limit < 0 || query.offset < 0 { return Err("Invalid history pagination".into()); }
    let mut result = Vec::new();
    let mut remaining = query.limit;
    let mut page = query.clone();
    while remaining > 0 {
        page.limit = remaining.min(1000);
        let entries = history.query_authorized(&page, permit, now()).map_err(|error| error.to_string())?;
        let count = i32::try_from(entries.len()).map_err(|_| "Invalid history pagination")?;
        result.extend(entries);
        if count < page.limit || query.id.is_some() || query.latest { break; }
        remaining -= count;
        page.offset = page.offset.checked_add(count).ok_or("Invalid history pagination")?;
    }
    history.privacy().check_read(permit, now()).map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_permission_cannot_be_reused_for_terminal_reads_even_for_zero_rows() {
        let directory = std::env::temp_dir().join(format!("dzc-terminal-permit-{}", uuid::Uuid::new_v4()));
        let history = HistoryService::open(&directory.join("synthetic.db")).unwrap();
        let original = permit(&history).unwrap();
        history.privacy().set_session(crate::api::PrivacySessionCommand::On { duration_ms: None }, now()).unwrap();
        assert_eq!(query_with_permit(&history, &HistoryQuery { limit: 0, ..Default::default() }, &original).unwrap_err(), "PrivacyEpochChanged");
        assert!(query(&history, &HistoryQuery { limit: 1001, ..Default::default() }).unwrap().is_empty());
        assert!(query(&history, &HistoryQuery { limit: 0, offset: -1, ..Default::default() }).is_err());
        drop(history);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
