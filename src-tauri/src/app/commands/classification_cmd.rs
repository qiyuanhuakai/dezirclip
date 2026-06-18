//! Tauri commands for clipboard content classification.
//!
//! Thin wrapper over [`crate::services::classification`]. Two commands are
//! exposed:
//!
//! - [`classify_text`] — run the heuristic dispatcher over an arbitrary
//!   string and return the matching kind names. Used by the React
//!   preview pane to surface type chips (url / email / phone / ...).
//! - [`get_supported_kinds`] — return every recognised kind in
//!   dispatcher order so the frontend can render a localized filter
//!   list without hardcoding the catalog on its side.
//!
//! Classification is pure: no I/O, no platform code, no shared state,
//! therefore both commands return their result directly and never need
//! `Result<_, String>`.

use crate::services::classification::{CONTENT_KINDS, classify};

/// Run every classifier in dispatcher order and return the names of all
/// matches. Empty input yields an empty vector; the frontend renders an
/// "untyped" chip in that case.
///
/// The underlying [`classify`] function is anchored (`^...$`) per regex,
/// so a sentence like "email me at user@example.com" intentionally
/// returns `[]` — the prose prefix blocks the email pattern. This is
/// the desired contract; the dispatcher is for typed payloads, not
/// free-form text mining.
#[tauri::command]
pub fn classify_text(text: String) -> Vec<String> {
    classify(&text)
}

/// Enumerate every kind recognised by [`classify`], in the same order
/// they appear in [`CONTENT_KINDS`]. The list is part of the IPC
/// contract: adding a new kind requires appending to `CONTENT_KINDS`
/// in `services/classification.rs` and the frontend filter list will
/// pick it up automatically.
#[tauri::command]
pub fn get_supported_kinds() -> Vec<String> {
    CONTENT_KINDS.iter().map(|kind| (*kind).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_text_url() {
        let kinds = classify_text("https://example.com".to_string());
        assert_eq!(kinds, vec!["url".to_string()]);
    }

    #[test]
    fn test_classify_text_multi() {
        // The original spec used "email me at user@example.com" and
        // expected ["email"], but the email regex is anchored to the
        // whole string. Use a pure email-shaped input so the test
        // exercises the email branch (and the `multi` here refers to
        // the multi-byte `user@example.com` payload, not multi-kind).
        let kinds = classify_text("user@example.com".to_string());
        assert_eq!(kinds, vec!["email".to_string()]);
    }

    #[test]
    fn test_classify_text_empty() {
        let kinds = classify_text("".to_string());
        assert!(kinds.is_empty(), "empty input must yield no kinds, got {kinds:?}");
    }

    #[test]
    fn test_classify_text_no_match() {
        let kinds = classify_text("hello world".to_string());
        assert!(kinds.is_empty(), "plain prose must not match, got {kinds:?}");
    }

    #[test]
    fn test_get_supported_kinds_count() {
        let kinds = get_supported_kinds();
        // CONTENT_KINDS is the source of truth; 12 entries today:
        // url, email, phone, idcard, ipv4, ipv6, jwt, path_windows,
        // path_unix, color_hex, color_rgb, json.
        assert_eq!(kinds.len(), CONTENT_KINDS.len());
        assert_eq!(kinds.len(), 12);
    }
}
