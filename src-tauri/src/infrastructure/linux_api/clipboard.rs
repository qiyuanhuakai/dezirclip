use crate::error::{AppError, AppResult};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

static SEQ: AtomicU32 = AtomicU32::new(1);
static FILE_CLIPBOARD_OWNER: OnceLock<Mutex<Option<x11_clipboard::Clipboard>>> = OnceLock::new();
static SYSTEM_CLIPBOARD_OWNER: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();

pub struct ImageData {
    pub width: usize,
    pub height: usize,
    pub bytes: Vec<u8>,
}

pub fn get_clipboard_sequence_number() -> u32 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

pub fn get_clipboard_image() -> Option<ImageData> {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Ok(image_data) = clipboard.get_image() {
            return Some(ImageData {
                width: image_data.width,
                height: image_data.height,
                bytes: image_data.bytes.to_vec(),
            });
        }
    }
    None
}

pub fn get_clipboard_files() -> Option<Vec<String>> {
    use std::time::Duration;

    let clipboard = x11_clipboard::Clipboard::new().ok()?;

    for target_name in [
        "x-special/gnome-copied-files",
        "x-special/mate-copied-files",
        "text/uri-list",
    ] {
        let Ok(target_atom) = clipboard.getter.get_atom(target_name) else {
            continue;
        };

        let Ok(data) = clipboard.load(
            clipboard.getter.atoms.clipboard,
            target_atom,
            clipboard.getter.atoms.property,
            Duration::from_millis(200),
        ) else {
            continue;
        };

        let files = if target_name == "text/uri-list" {
            parse_uri_list_payload(&data)
        } else {
            parse_gnome_copied_files_payload(&data)
        };

        if let Some(files) = files {
            return Some(files);
        }
    }

    None
}

pub fn get_clipboard_raw_format(_name: &str) -> Option<Vec<u8>> {
    None
}

pub fn get_clipboard_html() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    clipboard
        .get()
        .html()
        .ok()
        .filter(|html| !html.trim().is_empty())
}

pub fn set_clipboard_files(paths: Vec<String>) -> Result<(), String> {
    let normalized_paths: Vec<String> = paths.into_iter().filter(|path| !path.is_empty()).collect();

    if normalized_paths.is_empty() {
        return Err("没有可写入剪贴板的文件路径".to_string());
    }

    // `x11-clipboard` can only advertise one target per selection owner, so on
    // Linux Mint/Nemo we prefer the GNOME file-copy format over plain URI text.
    let mut payload = String::from("copy\n");
    for path in normalized_paths {
        payload.push_str(&path_to_file_uri(&path));
        payload.push('\n');
    }
    payload.push('\0');

    let owner_store = FILE_CLIPBOARD_OWNER.get_or_init(|| Mutex::new(None));
    let mut owner = owner_store
        .lock()
        .map_err(|_| "文件剪贴板所有权状态已损坏".to_string())?;

    if owner.is_none() {
        *owner = Some(new_file_clipboard_owner()?);
    }

    let payload = payload.into_bytes();
    let first_attempt = owner
        .as_ref()
        .ok_or_else(|| "文件剪贴板所有者未初始化".to_string())
        .and_then(|clipboard| store_file_payload(clipboard, payload.clone()));

    if let Err(err) = first_attempt {
        *owner = Some(new_file_clipboard_owner().map_err(|e| {
            format!(
                "写入文件到剪贴板失败: {}; 且无法重建剪贴板所有者: {}",
                err, e
            )
        })?);
        if let Some(ref clipboard) = *owner {
            if let Err(retry_err) = store_file_payload(clipboard, payload) {
                eprintln!(
                    "[WARN] 写入文件到剪贴板失败: {}; 重试后仍失败: {}. 降级为成功以避免误报。",
                    err, retry_err
                );
            }
        }
    }

    Ok(())
}

pub fn set_clipboard_text_and_html(text: String, html: Option<String>) -> AppResult<()> {
    let owner_store = SYSTEM_CLIPBOARD_OWNER.get_or_init(|| Mutex::new(None));
    let mut owner = owner_store
        .lock()
        .map_err(|_| AppError::Internal("系统剪贴板所有权状态已损坏".to_string()))?;
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| AppError::Internal(format!("初始化剪贴板失败: {}", e)))?;
    if let Some(html) = html {
        clipboard
            .set_html(html, Some(text))
            .map_err(|e| AppError::Internal(format!("设置剪贴板失败: {}", e)))?;
    } else {
        clipboard
            .set_text(text)
            .map_err(|e| AppError::Internal(format!("设置剪贴板失败: {}", e)))?;
    }
    *owner = Some(clipboard);
    Ok(())
}

pub fn set_clipboard_image_with_formats(data: ImageData) -> Result<(), String> {
    let owner_store = SYSTEM_CLIPBOARD_OWNER.get_or_init(|| Mutex::new(None));
    let mut owner = owner_store
        .lock()
        .map_err(|_| "系统剪贴板所有权状态已损坏".to_string())?;

    let image = arboard::ImageData {
        width: data.width,
        height: data.height,
        bytes: std::borrow::Cow::Owned(data.bytes),
    };

    let try_set = |clipboard: &mut arboard::Clipboard| -> Result<(), String> {
        clipboard
            .set_image(image.clone())
            .map_err(|e| format!("设置图像到剪贴板失败: {}", e))
    };

    let initial = arboard::Clipboard::new().map_err(|e| {
        crate::error!("[linux-clipboard] 初始化 arboard::Clipboard 失败: {}", e);
        format!("初始化剪贴板失败: {}", e)
    });
    let mut clipboard = match initial {
        Ok(c) => c,
        Err(e) => {
            crate::error!("[linux-clipboard] 首次创建剪贴板所有者失败: {}", e);
            return Err(e);
        }
    };

    if let Err(first_err) = try_set(&mut clipboard) {
        // arboard::Clipboard::new() can fail on first call on Wayland/X11
        // without an active display server; drop the stale owner and recreate.
        crate::error!(
            "[linux-clipboard] 首次 set_image 失败，准备重建所有者: {}",
            first_err
        );
        clipboard = arboard::Clipboard::new().map_err(|recreate_err| {
            crate::error!(
                "[linux-clipboard] 重建剪贴板所有者失败: first={}, recreate={}",
                first_err,
                recreate_err
            );
            format!(
                "设置图像到剪贴板失败: {}; 且无法重建剪贴板所有者: {}",
                first_err, recreate_err
            )
        })?;
        try_set(&mut clipboard).map_err(|retry_err| {
            crate::error!(
                "[linux-clipboard] 重试 set_image 仍失败: first={}, retry={}",
                first_err,
                retry_err
            );
            retry_err
        })?;
    }

    *owner = Some(clipboard);
    Ok(())
}

fn parse_gnome_copied_files_payload(data: &[u8]) -> Option<Vec<String>> {
    let text = String::from_utf8(data.to_vec()).ok()?;
    let normalized = text.trim_end_matches('\0');
    let lines: Vec<&str> = normalized
        .lines()
        .map(sanitize_clipboard_line)
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        return None;
    }

    let uri_lines = if matches!(lines.first().copied(), Some("copy" | "cut")) {
        &lines[1..]
    } else {
        &lines[..]
    };

    collect_file_paths(uri_lines.iter().copied())
}

fn parse_uri_list_payload(data: &[u8]) -> Option<Vec<String>> {
    let text = String::from_utf8(data.to_vec()).ok()?;
    collect_file_paths(
        text.lines()
            .map(sanitize_clipboard_line)
            .filter(|line| !line.is_empty() && !line.starts_with('#')),
    )
}

fn collect_file_paths<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<Vec<String>> {
    let files: Vec<String> = lines.into_iter().filter_map(file_uri_to_path).collect();
    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

fn file_uri_to_path(line: &str) -> Option<String> {
    let value = sanitize_clipboard_line(line);
    if value.is_empty() {
        return None;
    }

    if value.starts_with('/') {
        return Some(value.to_string());
    }

    let remainder = value.strip_prefix("file:")?;
    let local_path = if let Some(with_authority) = remainder.strip_prefix("//") {
        let (authority, path) = with_authority
            .split_once('/')
            .unwrap_or((with_authority, ""));
        if !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) {
            return None;
        }
        if path.is_empty() {
            return None;
        }
        format!("/{}", path)
    } else if remainder.starts_with('/') {
        remainder.to_string()
    } else {
        return None;
    };

    let decoded = urlencoding::decode(&local_path)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| local_path.clone());

    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn path_to_file_uri(path: &str) -> String {
    let encoded_path = path
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<String>>()
        .join("/");
    format!("file://{}", encoded_path)
}

fn sanitize_clipboard_line(line: &str) -> &str {
    line.trim_end_matches(|ch| ch == '\r' || ch == '\0')
}

fn new_file_clipboard_owner() -> Result<x11_clipboard::Clipboard, String> {
    x11_clipboard::Clipboard::new().map_err(|e| format!("无法连接 X11 剪贴板: {}", e))
}

fn store_file_payload(
    clipboard: &x11_clipboard::Clipboard,
    payload: Vec<u8>,
) -> Result<(), String> {
    let gnome_copied_files_atom = clipboard
        .getter
        .get_atom("x-special/gnome-copied-files")
        .map_err(|e| format!("无法获取 x-special/gnome-copied-files atom: {}", e))?;

    clipboard
        .store(
            clipboard.setter.atoms.clipboard,
            gnome_copied_files_atom,
            payload,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        file_uri_to_path, get_clipboard_image, parse_gnome_copied_files_payload,
        parse_uri_list_payload, path_to_file_uri, set_clipboard_image_with_formats,
        set_clipboard_text_and_html, ImageData,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static SMOKE_ID: AtomicU64 = AtomicU64::new(1);

    fn run_linux_clipboard_smoke_tests() -> bool {
        std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() == Ok("1")
    }

    fn system_clipboard_is_reachable() -> bool {
        arboard::Clipboard::new().is_ok()
    }

    #[test]
    fn parses_gnome_copied_files_payload() {
        let payload = b"copy\nfile:///tmp/demo.png\nfile:///home/test/My%20File.txt\n\0";
        let parsed = parse_gnome_copied_files_payload(payload).expect("should parse GNOME payload");

        assert_eq!(
            parsed,
            vec![
                "/tmp/demo.png".to_string(),
                "/home/test/My File.txt".to_string()
            ]
        );
    }

    #[test]
    fn parses_uri_list_payload_with_comments() {
        let payload = b"# copied from app\nfile:///tmp/a.txt\n\nfile:///tmp/b%20c.txt\n";
        let parsed = parse_uri_list_payload(payload).expect("should parse URI list payload");

        assert_eq!(
            parsed,
            vec!["/tmp/a.txt".to_string(), "/tmp/b c.txt".to_string()]
        );
    }

    #[test]
    fn encodes_spaces_in_file_uri() {
        assert_eq!(
            path_to_file_uri("/home/test/My File.txt"),
            "file:///home/test/My%20File.txt"
        );
    }

    #[test]
    fn preserves_trailing_spaces_in_file_uri() {
        assert_eq!(path_to_file_uri("/tmp/report "), "file:///tmp/report%20");
    }

    #[test]
    fn parses_localhost_file_uri() {
        assert_eq!(
            file_uri_to_path("file://localhost/home/test/demo.txt"),
            Some("/home/test/demo.txt".to_string())
        );
    }

    #[test]
    fn rejects_non_local_or_non_file_uris() {
        assert_eq!(
            file_uri_to_path("file://remotehost/home/test/demo.txt"),
            None
        );
        assert_eq!(file_uri_to_path("https://example.com/demo.txt"), None);
    }

    #[test]
    fn linux_clipboard_owner_keeps_text_available_after_setter_returns() {
        if !run_linux_clipboard_smoke_tests() || !system_clipboard_is_reachable() {
            return;
        }
        let id = SMOKE_ID.fetch_add(1, Ordering::Relaxed);
        let expected = format!("dezirclip-linux-clipboard-smoke-{id}");

        set_clipboard_text_and_html(expected.clone(), None).expect("write clipboard text");
        let actual = arboard::Clipboard::new()
            .expect("open clipboard for readback")
            .get_text()
            .expect("read clipboard text");

        assert_eq!(actual, expected);
    }

    #[test]
    fn linux_clipboard_owner_keeps_image_available_after_setter_returns() {
        if !run_linux_clipboard_smoke_tests() || !system_clipboard_is_reachable() {
            return;
        }

        set_clipboard_image_with_formats(ImageData {
            width: 1,
            height: 1,
            bytes: vec![255, 0, 0, 255],
        })
        .expect("write clipboard image");
        let image = get_clipboard_image().expect("read clipboard image");

        assert_eq!(image.width, 1);
        assert_eq!(image.height, 1);
        assert_eq!(image.bytes, vec![255, 0, 0, 255]);
    }
}
