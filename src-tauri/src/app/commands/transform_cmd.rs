use dzc_core::transforms::{self, TransformKindDto};

#[tauri::command]
pub fn transform_text(text: String, kind: String) -> Result<String, String> {
    transforms::transform_text(&text, &kind)
}

#[tauri::command]
pub fn list_transform_kinds() -> Vec<TransformKindDto> {
    transforms::list_transform_kinds()
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
        let upper = transform_text("你好 こんにちは 🚀".to_string(), "to_uppercase".to_string())
            .expect("uppercase should not fail");
        assert_eq!(upper, "你好 こんにちは 🚀");

        // reverse_text is char-wise, not byte-wise, so multi-byte CJK stays
        // intact and the order of `char`s is flipped.
        let reversed = transform_text("你好abc".to_string(), "reverse_text".to_string())
            .expect("reverse should not fail");
        assert_eq!(reversed, "cba好你");
    }
}
