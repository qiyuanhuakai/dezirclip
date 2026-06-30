pub fn simulate_paste() -> Result<(), String> {
    simulate_paste_with_method("ctrl_v", None)
}

pub fn simulate_paste_with_method(method: &str, content_type: Option<&str>) -> Result<(), String> {
    simulate_paste_with_method_and_content(method, content_type, None)
}

pub fn simulate_paste_with_method_and_content(
    method: &str,
    content_type: Option<&str>,
    expected_text: Option<&str>,
) -> Result<(), String> {
    if let Some(text) = expected_text.filter(|_| content_type_is_text_like(content_type)) {
        wait_for_clipboard_text(text);
    }

    let key_combo = match resolve_effective_paste_method(method, content_type) {
        "ctrl_v" => "ctrl+v",
        _ => "Shift+Insert",
    };

    release_active_modifiers();

    let status = std::process::Command::new("xdotool")
        .args([&"key", key_combo])
        .status()
        .map_err(|e| format!("xdotool 粘贴失败: {}", e))?;

    if !status.success() {
        eprintln!(
            "xdotool warning: exit code {:?}, but keystroke may still have been delivered",
            status.code()
        );
    }

    Ok(())
}

fn content_type_is_text_like(content_type: Option<&str>) -> bool {
    matches!(content_type, Some("text" | "code" | "url" | "rich_text"))
}

fn wait_for_clipboard_text(expected: &str) {
    for _ in 0..8 {
        if arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.get_text())
            .map(|text| text == expected)
            .unwrap_or(false)
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn release_active_modifiers() {
    // Linux hotkeys can leave Alt/Ctrl/Shift physically active while we synthesize
    // the paste keystroke; release them explicitly so Ctrl+V/Shift+Insert is not
    // transformed into a different shortcut by the target app.
    let _ = std::process::Command::new("xdotool")
        .args([
            "keyup",
            "Control_L",
            "Control_R",
            "Shift_L",
            "Shift_R",
            "Alt_L",
            "Alt_R",
            "Super_L",
            "Super_R",
            "Meta_L",
            "Meta_R",
        ])
        .status();
}

pub fn simulate_paste_sequence(texts: Vec<String>) -> Result<(), String> {
    for (i, text) in texts.iter().enumerate() {
        set_clipboard_text(text)?;
        simulate_paste_with_method("shift_insert", Some("text"))?;

        if i < texts.len() - 1 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    Ok(())
}

fn set_clipboard_text(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("初始化剪贴板失败: {}", e))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("设置剪贴板失败: {}", e))?;
    Ok(())
}

fn resolve_effective_paste_method(method: &str, content_type: Option<&str>) -> &'static str {
    let text_like = matches!(content_type, Some("text" | "code" | "url" | "rich_text"));

    if method == "ctrl_v" {
        "ctrl_v"
    } else if text_like {
        "shift_insert"
    } else {
        "ctrl_v"
    }
}

#[cfg(test)]
mod tests {
    use super::{content_type_is_text_like, resolve_effective_paste_method};

    #[test]
    fn keeps_explicit_ctrl_v() {
        assert_eq!(
            resolve_effective_paste_method("ctrl_v", Some("text")),
            "ctrl_v"
        );
    }

    #[test]
    fn prefers_shift_insert_for_text_like_content() {
        assert_eq!(
            resolve_effective_paste_method("shift_insert", Some("rich_text")),
            "shift_insert"
        );
    }

    #[test]
    fn keeps_ctrl_v_for_file_like_content() {
        assert_eq!(
            resolve_effective_paste_method("shift_insert", Some("file")),
            "ctrl_v"
        );
        assert_eq!(
            resolve_effective_paste_method("shift_insert", Some("image")),
            "ctrl_v"
        );
    }

    #[test]
    fn identifies_text_like_content_for_clipboard_settle_wait() {
        assert!(content_type_is_text_like(Some("text")));
        assert!(content_type_is_text_like(Some("rich_text")));
        assert!(!content_type_is_text_like(Some("image")));
        assert!(!content_type_is_text_like(None));
    }
}
