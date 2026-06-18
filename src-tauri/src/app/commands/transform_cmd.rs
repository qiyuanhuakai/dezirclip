//! Tauri commands for text transforms.
//!
//! Thin wrapper over [`crate::services::transforms`]. Two commands are exposed:
//!
//! - [`transform_text`] — apply a transform to a string. Used by the
//!   "edit before paste" feature to normalize / reformat clipboard content.
//! - [`list_transform_kinds`] — return every kind plus its zh/en labels so
//!   the React settings panel can render a localized menu without hardcoding
//!   the list on the frontend.

use crate::services::transforms::{TransformError, TransformKind, apply_transform};
use serde::Serialize;

/// JSON-safe description of a single transform kind. Returned in batches by
/// [`list_transform_kinds`]. The `id` is the snake_case discriminator the
/// frontend sends back to [`transform_text`].
#[derive(Debug, Clone, Serialize)]
pub struct TransformKindDto {
    pub id: String,
    pub label_zh: String,
    pub label_en: String,
}

// Localized labels, kept in lockstep with `TransformKind::all()`. Index N
// pairs with the Nth variant. The order of the slices must NOT diverge
// from `TransformKind::all()` or `list_transform_kinds` will return the
// wrong label for a given id.
const LABELS_ZH: &[&str] = &[
    "转大写",
    "转小写",
    "首字母大写",
    "句首大写",
    "去除空格",
    "合并连续空格",
    "移除换行",
    "升序排序",
    "降序排序",
    "去重行",
    "反转行序",
    "反转文本",
    "添加行号",
    "URL 编码",
    "URL 解码",
    "Base64 编码",
    "Base64 解码",
];

const LABELS_EN: &[&str] = &[
    "To Uppercase",
    "To Lowercase",
    "Title Case",
    "Sentence Case",
    "Trim",
    "Collapse Spaces",
    "Remove Newlines",
    "Sort Asc",
    "Sort Desc",
    "Dedupe",
    "Reverse Lines",
    "Reverse Text",
    "Line Numbers",
    "URL Encode",
    "URL Decode",
    "Base64 Encode",
    "Base64 Decode",
];

/// Resolve a snake_case `kind` string to a [`TransformKind`]. Returns
/// `None` for unknown ids so the caller can return a frontend-friendly
/// error string instead of panicking on a bad IPC payload.
fn parse_kind(kind: &str) -> Option<TransformKind> {
    TransformKind::all()
        .iter()
        .copied()
        .find(|candidate| candidate.to_string() == kind)
}

/// Apply `kind` to `text`. Returns the transformed string on success, or
/// a human-readable error message on either an unknown `kind` or a
/// decoder failure (e.g. malformed URL percent-escapes, bad base64).
#[tauri::command]
pub fn transform_text(text: String, kind: String) -> Result<String, String> {
    let kind = parse_kind(&kind).ok_or_else(|| format!("unknown transform kind: {kind}"))?;
    apply_transform(&text, kind).map_err(|e: TransformError| e.to_string())
}

/// Enumerate every transform kind with its localized labels. The list is
/// stable — order and contents are part of the IPC contract with the
/// frontend, so adding a new kind requires bumping the count and appending
/// to both `LABELS_ZH` and `LABELS_EN`.
#[tauri::command]
pub fn list_transform_kinds() -> Vec<TransformKindDto> {
    let all = TransformKind::all();
    debug_assert_eq!(all.len(), TransformKind::COUNT);
    debug_assert_eq!(all.len(), LABELS_ZH.len());
    debug_assert_eq!(all.len(), LABELS_EN.len());
    all.iter()
        .enumerate()
        .map(|(idx, kind)| TransformKindDto {
            id: kind.to_string(),
            label_zh: LABELS_ZH[idx].to_string(),
            label_en: LABELS_EN[idx].to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_transform_uppercase() {
        let result = transform_text("hello".to_string(), "to_uppercase".to_string());
        assert_eq!(result, Ok("HELLO".to_string()));
    }

    #[test]
    fn test_transform_invalid_kind() {
        let result = transform_text("hello".to_string(), "not_a_kind".to_string());
        assert!(result.is_err());
        // The error must mention the offending kind so the frontend can show
        // an actionable message instead of a generic "transform failed".
        assert!(result.unwrap_err().contains("not_a_kind"));
    }

    #[test]
    fn test_list_kinds_count() {
        let kinds = list_transform_kinds();
        assert_eq!(kinds.len(), 17);
        // Spot-check: every id is non-empty and every label is non-empty.
        for dto in &kinds {
            assert!(!dto.id.is_empty());
            assert!(!dto.label_zh.is_empty());
            assert!(!dto.label_en.is_empty());
        }
    }

    #[test]
    fn test_list_kinds_unique_ids() {
        let kinds = list_transform_kinds();
        let mut seen: HashSet<String> = HashSet::new();
        for dto in &kinds {
            assert!(seen.insert(dto.id.clone()), "duplicate id: {}", dto.id);
        }
        assert_eq!(seen.len(), 17);
    }

    #[test]
    fn test_transform_unicode_cjk_emoji() {
        // CJK characters have no case mapping — uppercase is a no-op.
        let upper =
            transform_text("你好 こんにちは 🚀".to_string(), "to_uppercase".to_string())
                .expect("uppercase should not fail");
        assert_eq!(upper, "你好 こんにちは 🚀");

        // reverse_text is char-wise, not byte-wise, so multi-byte CJK stays
        // intact and the order of `char`s is flipped.
        let reversed =
            transform_text("你好abc".to_string(), "reverse_text".to_string())
                .expect("reverse should not fail");
        assert_eq!(reversed, "cba好你");
    }
}
