//! Only bounded, typed input-state diagnostics may leave the child's stderr.
use serde_json::Value;

const PREFIX: &[u8] = b"[surface-input] ";
const MAX_LINE: usize = 512;

fn sanitize(line: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(line.strip_prefix(PREFIX)?).ok()?;
    let fields = value.as_object()?;
    let stage = fields.get("stage")?.as_str()?;
    let booleans: &[&str] = match stage {
        "enter_gate" => &["visible", "same_display", "modifiers_clear", "native_text_current"],
        "attempt" | "input_changed" | "selection_unavailable" => &[
            "visible", "input_current", "query_current", "loading", "composing", "pending", "selectable",
        ],
        "dispatch_requested" | "write_begin" | "write_done" | "write_failed"
        | "result_performed" | "result_not_performed" => &[],
        _ => return None,
    };
    let selection = matches!(stage, "attempt" | "input_changed" | "selection_unavailable");
    if fields.len() != 1 + booleans.len() + if selection { 2 } else { 0 }
        || booleans.iter().any(|name| !fields.get(*name).is_some_and(Value::is_boolean)) {
        return None;
    }
    if selection {
        let selected = fields.get("selected")?.as_i64()?;
        let count = fields.get("count")?.as_i64()?;
        if !(-1..=1000).contains(&selected) || !(0..=1000).contains(&count) { return None; }
    }
    serde_json::to_string(&value).ok()
}

#[derive(Default)]
pub(super) struct ChildInput {
    line: Vec<u8>,
    oversized: bool,
    emitted: u8,
}
impl ChildInput {
    pub fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(String)) {
        for &byte in bytes {
            if byte == b'\n' {
                if !self.oversized && self.emitted < 64 {
                    if let Some(message) = sanitize(&self.line) {
                        self.emitted += 1;
                        emit(message);
                    }
                }
                self.line.clear();
                self.oversized = false;
            } else if !self.oversized {
                if self.line.len() < MAX_LINE - 1 { self.line.push(byte); }
                else { self.line.clear(); self.oversized = true; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unknown_fields_stages_and_content_values() {
        for line in [
            r#"[surface-input] {"stage":"secret"}"#,
            r#"[surface-input] {"stage":"write_done","query":"secret"}"#,
            r#"[surface-input] {"stage":"enter_gate","visible":"secret","same_display":true,"modifiers_clear":true,"native_text_current":true}"#,
            r#"unrelated child diagnostics"#,
        ] { assert!(sanitize(line.as_bytes()).is_none()); }
    }
    #[test]
    fn selection_shape_requires_only_bounded_numbers_and_booleans() {
        let mut value = serde_json::json!({"stage":"attempt","visible":true,"input_current":true,
            "query_current":true,"loading":false,"composing":false,"pending":false,"selectable":true,
            "selected":0,"count":1});
        let line = |value: &Value| format!("[surface-input] {value}");
        assert!(sanitize(line(&value).as_bytes()).is_some());
        value["count"] = 1001.into();
        assert!(sanitize(line(&value).as_bytes()).is_none());
        value["count"] = 1.into(); value["selected"] = (-2).into();
        assert!(sanitize(line(&value).as_bytes()).is_none());
        value["selected"] = 0.into(); value.as_object_mut().unwrap().remove("loading");
        assert!(sanitize(line(&value).as_bytes()).is_none());
    }
    #[test]
    fn framing_handles_chunks_and_discards_whole_oversized_lines() {
        let mut decoder = ChildInput::default();
        let mut messages = Vec::new();
        decoder.feed(b"[surface-input] {\"stage\":", |message| messages.push(message));
        assert!(messages.is_empty());
        decoder.feed(b"\"write_begin\"}\n", |message| messages.push(message));
        assert_eq!(messages.len(), 1);
        decoder.feed(&vec![b'x'; 1024], |message| messages.push(message));
        decoder.feed(b"[surface-input] {\"stage\":\"write_done\"}\n", |message| messages.push(message));
        assert_eq!(messages.len(), 1);
        decoder.feed(b"[surface-input] {\"stage\":\"write_done\"}\n", |message| messages.push(message));
        assert_eq!(messages.len(), 2);
    }
    #[test]
    fn output_is_capped_for_the_entire_child_lifetime() {
        let mut decoder = ChildInput::default();
        let mut count = 0;
        for _ in 0..100 {
            decoder.feed(b"[surface-input] {\"stage\":\"write_done\"}\n", |_| count += 1);
        }
        assert_eq!(count, 64);
        assert!(decoder.line.is_empty());
    }
}
