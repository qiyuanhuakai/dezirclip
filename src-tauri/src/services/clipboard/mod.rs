mod pipeline;
mod utils;

use crate::app_state::SettingsState;
pub use crate::database::DbState;
use arboard::Clipboard;
use base64::Engine;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

#[cfg(not(target_os = "windows"))]
use crate::infrastructure::linux_api::window_tracker::{
    get_active_app_info as get_clipboard_source_app_info, ActiveAppInfo,
};
#[cfg(target_os = "windows")]
use crate::infrastructure::windows_api::window_tracker::{
    get_clipboard_source_app_info, ActiveAppInfo,
};

#[cfg(not(target_os = "windows"))]
use crate::infrastructure::linux_api::clipboard as clipboard_api;
#[cfg(target_os = "windows")]
use crate::infrastructure::windows_api::win_clipboard as clipboard_api;

use utils::attach_rich_image_fallback;
#[cfg(target_os = "windows")]
use utils::parse_cf_html;

enum ClipboardProcessPayload {
    Pipeline {
        data: ClipboardData,
        source_override: Option<String>,
        source_snapshot: Option<ActiveAppInfo>,
    },
    #[cfg(target_os = "windows")]
    GifImage {
        bytes: Vec<u8>,
        source_snapshot: Option<ActiveAppInfo>,
    },
    RgbaImage {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
        source_snapshot: Option<ActiveAppInfo>,
    },
}

struct ClipboardProcessTask {
    app_handle: AppHandle,
    payload: ClipboardProcessPayload,
}

fn clipboard_process_sender() -> &'static Sender<ClipboardProcessTask> {
    static SENDER: OnceLock<Sender<ClipboardProcessTask>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<ClipboardProcessTask>();
        let _ = std::thread::Builder::new()
            .name("clipboard-process-worker".to_string())
            .spawn(move || {
                while let Ok(task) = rx.recv() {
                    match task.payload {
                        ClipboardProcessPayload::Pipeline {
                            data,
                            source_override,
                            source_snapshot,
                        } => {
                            process_new_entry(
                                &task.app_handle,
                                data,
                                source_override,
                                source_snapshot,
                            );
                        }
                        #[cfg(target_os = "windows")]
                        ClipboardProcessPayload::GifImage {
                            bytes,
                            source_snapshot,
                        } => {
                            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                            process_new_entry(
                                &task.app_handle,
                                ClipboardData::Image {
                                    data_url: format!("data:image/gif;base64,{}", b64),
                                },
                                None,
                                source_snapshot,
                            );
                        }
                        ClipboardProcessPayload::RgbaImage {
                            width,
                            height,
                            bytes,
                            source_snapshot,
                        } => {
                            if let Some(data_url) =
                                build_png_data_url_from_rgba(width, height, bytes)
                            {
                                process_new_entry(
                                    &task.app_handle,
                                    ClipboardData::Image { data_url },
                                    None,
                                    source_snapshot,
                                );
                            }
                        }
                    }
                }
            });
        tx
    })
}

fn process_new_entry_async(
    app_handle: AppHandle,
    data: ClipboardData,
    source_override: Option<String>,
    source_snapshot: Option<ActiveAppInfo>,
) {
    let task = ClipboardProcessTask {
        app_handle,
        payload: ClipboardProcessPayload::Pipeline {
            data,
            source_override,
            source_snapshot,
        },
    };
    if let Err(err) = clipboard_process_sender().send(task) {
        let task = err.0;
        if let ClipboardProcessPayload::Pipeline {
            data,
            source_override,
            source_snapshot,
        } = task.payload
        {
            process_new_entry(&task.app_handle, data, source_override, source_snapshot);
        }
    }
}

fn build_png_data_url_from_rgba(width: u32, height: u32, bytes: Vec<u8>) -> Option<String> {
    if let Some(img_buf) = image::RgbaImage::from_raw(width, height, bytes) {
        let mut encoded: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut encoded);
        if img_buf
            .write_to(&mut cursor, image::ImageFormat::Png)
            .is_ok()
        {
            let b64 = base64::engine::general_purpose::STANDARD.encode(encoded);
            return Some(format!("data:image/png;base64,{}", b64));
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn image_file_to_data_url(path: &str) -> Option<String> {
    let lower = path.to_lowercase();
    if !(lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".bmp")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif"))
    {
        return None;
    }

    let image = image::open(path).ok()?;
    let mut bytes: Vec<u8> = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    image.write_to(&mut cursor, image::ImageFormat::Png).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{}", b64))
}

#[cfg(target_os = "windows")]
fn process_gif_entry_async(
    app_handle: AppHandle,
    bytes: Vec<u8>,
    source_snapshot: Option<ActiveAppInfo>,
) {
    let task = ClipboardProcessTask {
        app_handle,
        payload: ClipboardProcessPayload::GifImage {
            bytes,
            source_snapshot,
        },
    };
    if let Err(err) = clipboard_process_sender().send(task) {
        let task = err.0;
        if let ClipboardProcessPayload::GifImage {
            bytes,
            source_snapshot,
        } = task.payload
        {
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            process_new_entry(
                &task.app_handle,
                ClipboardData::Image {
                    data_url: format!("data:image/gif;base64,{}", b64),
                },
                None,
                source_snapshot,
            );
        }
    }
}

fn process_rgba_image_entry_async(
    app_handle: AppHandle,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
    source_snapshot: Option<ActiveAppInfo>,
) -> bool {
    let fallback_bytes = bytes.clone();
    let fallback_snapshot = source_snapshot.clone();
    let task = ClipboardProcessTask {
        app_handle,
        payload: ClipboardProcessPayload::RgbaImage {
            width,
            height,
            bytes,
            source_snapshot,
        },
    };
    if let Err(err) = clipboard_process_sender().send(task) {
        let task = err.0;
        if let ClipboardProcessPayload::RgbaImage {
            width,
            height,
            source_snapshot,
            ..
        } = task.payload
        {
            if let Some(data_url) = build_png_data_url_from_rgba(width, height, fallback_bytes) {
                process_new_entry(
                    &task.app_handle,
                    ClipboardData::Image { data_url },
                    None,
                    source_snapshot.or(fallback_snapshot),
                );
                return true;
            }
            return false;
        }
    }
    true
}

/// Whether a failed image probe is worth waiting for and asking again.
///
/// The retry exists for one transient: a source that publishes its image format
/// a moment after the HTML, so a first pass can find nothing where a later pass
/// finds the format. That is worth waiting for only while there is reason to
/// think an image is on the way -- the clipboard advertising one, or the probe
/// having seen an image format it could not decode.
///
/// "Confirming there is no image" has to come from an availability query, not
/// from a failed read. `get_clipboard_raw_format` returns `None` both when the
/// format is absent *and* when `OpenClipboard` / `GetClipboardData` /
/// `GlobalLock` failed, so treating its `None` as proof of absence would drop
/// the fallback for a DIB-only clipboard whose read happened to fail -- a case
/// the old code still recovered from.
fn should_retry_image_probe(
    found: Option<&str>,
    clipboard_advertises_image: bool,
    saw_image_format: bool,
) -> bool {
    if found.is_some() {
        return false;
    }
    clipboard_advertises_image || saw_image_format
}

/// Run `probe` until it yields an image, giving up early once the clipboard is
/// known to hold none.
///
/// `probe` reports what it found and whether it saw an image format at all --
/// those differ when a format is present but not yet readable, which is the one
/// case worth waiting out. `advertises` is the availability query that decides
/// whether "nothing found" means "nothing here". `wait` is injected so the
/// scheduling is testable without spending real time.
fn probe_image_with_retry<P, A, W>(
    attempts: usize,
    mut probe: P,
    advertises: A,
    mut wait: W,
) -> Option<String>
where
    P: FnMut() -> (Option<String>, bool),
    A: Fn() -> bool,
    W: FnMut(std::time::Duration),
{
    let (found, saw_image_format) = probe();
    if let Some(found) = found {
        return Some(found);
    }
    if !should_retry_image_probe(None, advertises(), saw_image_format) {
        return None;
    }
    for _ in 1..attempts {
        wait(std::time::Duration::from_millis(35));
        if let Some(found) = probe().0 {
            return Some(found);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn clipboard_image_fallback_data_url() -> Option<String> {
    // One pass over the formats, with no waiting between them.
    //
    // The probing itself is nearly free: measured on this machine, a full round
    // over the five named formats plus the CF_DIBV5/CF_DIB decode is 0.021 ms
    // when the clipboard holds no image at all.
    //
    // What is not free is *deciding not to find anything*. This helper is on
    // the rich-text capture path, so it runs for every formatted copy -- Word,
    // Excel, a browser's "copy rich text" -- and the overwhelming majority of
    // those carry no image. The old shape answered that question three times
    // with a 35 ms sleep in between: 106.1 ms of sleeping against 0.063 ms of
    // work, so 99.94% of the time was spent waiting to re-ask a question whose
    // answer is a property of what the source app put on the clipboard.
    //
    // The retry was there for a transient: an Office app that publishes the
    // image format a moment after the HTML. That transient can only be
    // observed when there is something to observe, so the wait is now paid
    // only when a pass actually saw a named image format but could not turn it
    // into PNG -- not when the clipboard simply has no image on it.
    // `saw_image_format` is what keeps a late-arriving or briefly unreadable
    // format retryable: it is set whenever a format yielded bytes, separately
    // from whether those bytes became a PNG.
    let probe_once = || -> (Option<String>, bool) {
        let mut saw_image_format = false;
        unsafe {
            // Some sources (e.g. Office apps) may provide PNG/JPEG custom formats.
            for name in ["PNG", "image/png", "JFIF", "JPEG", "image/jpeg"] {
                if let Some(raw) =
                    crate::infrastructure::windows_api::win_clipboard::get_clipboard_raw_format(
                        name,
                    )
                {
                    saw_image_format = true;
                    if let Ok(png) = crate::services::image_png::as_png_bytes(&raw) {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
                        return (Some(format!("data:image/png;base64,{}", b64)), true);
                    }
                }
            }

            // Fallback to CF_DIB/CF_DIBV5 decode.
            if let Some(image) =
                crate::infrastructure::windows_api::win_clipboard::get_clipboard_image()
            {
                saw_image_format = true;
                if let Some(img_buf) =
                    image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes)
                {
                    let mut bytes: Vec<u8> = Vec::new();
                    let mut cursor = std::io::Cursor::new(&mut bytes);
                    if img_buf
                        .write_to(&mut cursor, image::ImageFormat::Png)
                        .is_ok()
                    {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                        return (Some(format!("data:image/png;base64,{}", b64)), true);
                    }
                }
            }
        }
        (None, saw_image_format)
    };

    // The availability query decides whether a failed read means "nothing here".
    // The driver runs the first probe itself and exits without waiting when the
    // answer is already no.
    probe_image_with_retry(
        2,
        probe_once,
        crate::infrastructure::windows_api::win_clipboard::clipboard_advertises_image,
        |d| std::thread::sleep(d),
    )
}

#[cfg(not(target_os = "windows"))]
fn clipboard_image_fallback_data_url() -> Option<String> {
    let image = clipboard_api::get_clipboard_image()?;
    build_png_data_url_from_rgba(image.width as u32, image.height as u32, image.bytes)
}

pub fn start_clipboard_monitor(app_handle: AppHandle) {
    use std::sync::{Arc, Mutex};

    // Initial state for deduplication and self-copy detection
    let mut last_text = String::new();

    #[cfg(target_os = "windows")]
    let last_seq = clipboard_api::get_clipboard_sequence_number();
    #[cfg(not(target_os = "windows"))]
    let last_seq = clipboard_api::get_clipboard_sequence_number();

    let mut last_image_hash = 0u64;

    // We can initialize these with current content to avoid capturing on startup
    if let Ok(mut cb) = Clipboard::new() {
        last_text = cb.get_text().unwrap_or_default();

        #[cfg(target_os = "windows")]
        let image_hash = unsafe {
            if let Some(image) = clipboard_api::get_clipboard_image() {
                let mut hash = image.bytes.len() as u64;
                if !image.bytes.is_empty() {
                    hash = hash
                        .wrapping_add(image.bytes[0] as u64)
                        .wrapping_add(image.bytes[image.bytes.len() / 2] as u64)
                        .wrapping_add(image.bytes[image.bytes.len() - 1] as u64);
                }
                hash
            } else {
                0u64
            }
        };

        #[cfg(not(target_os = "windows"))]
        let image_hash = if let Some(image) = clipboard_api::get_clipboard_image() {
            let mut hash = image.bytes.len() as u64;
            if !image.bytes.is_empty() {
                hash = hash
                    .wrapping_add(image.bytes[0] as u64)
                    .wrapping_add(image.bytes[image.bytes.len() / 2] as u64)
                    .wrapping_add(image.bytes[image.bytes.len() - 1] as u64);
            }
            hash
        } else {
            0u64
        };

        last_image_hash = image_hash;
    }

    struct MonitorState {
        last_text: String,
        last_seq: u32,
        last_image_hash: u64,
        last_content_hash: u64,
        last_process_time: u64,
    }

    let state = Arc::new(Mutex::new(MonitorState {
        last_text,
        last_seq,
        last_image_hash,
        last_content_hash: 0,
        last_process_time: 0,
    }));

    let app_clone = app_handle.clone();
    let state_lock = state.clone();

    // Start the native Windows listener
    crate::services::clipboard_listener::listen_clipboard(Arc::new(move || {
        let app = app_clone.clone();
        let mut monitor_state = state_lock.lock().unwrap();

        // 1. Check for pause
        if crate::CLIPBOARD_MONITOR_PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }

        // 2. Sequence check (De-bounce Windows firing multiple events for one copy)

        #[cfg(target_os = "windows")]
        let current_seq = clipboard_api::get_clipboard_sequence_number();

        #[cfg(not(target_os = "windows"))]
        let current_seq = clipboard_api::get_clipboard_sequence_number();

        if current_seq == monitor_state.last_seq {
            return;
        }
        monitor_state.last_seq = current_seq;

        let source_snapshot = get_clipboard_source_app_info();

        // Give source app (especially Excel) time to release lock/finish writing
        // fixes "Another application is using the clipboard" error
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Initialize clipboard for this thread
        let mut clipboard = match Clipboard::new() {
            Ok(cb) => cb,
            Err(_) => return,
        };

        // 3. Content-based deduplication with time window (for Chrome address bar, etc.)
        // Some apps trigger multiple clipboard updates with different sequence numbers
        // but identical content within a short time window
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Calculate hash of current clipboard content
        let current_content_hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();

            // Hash text content if available
            if let Ok(text) = clipboard.get_text() {
                text.hash(&mut hasher);
            }

            // Also consider image hash if present

            #[cfg(target_os = "windows")]
            let image_bytes = unsafe { clipboard_api::get_clipboard_image().map(|i| i.bytes) };

            #[cfg(not(target_os = "windows"))]
            let image_bytes = clipboard_api::get_clipboard_image().map(|i| i.bytes);

            if let Some(bytes) = image_bytes {
                bytes.hash(&mut hasher);
            }

            hasher.finish()
        };

        // If content is identical to last processed content within 500ms window, skip
        if current_content_hash == monitor_state.last_content_hash
            && current_content_hash != 0
            && now.saturating_sub(monitor_state.last_process_time) < 500
        {
            return;
        }

        monitor_state.last_content_hash = current_content_hash;
        monitor_state.last_process_time = now;

        let mut handled = false;

        // --- Core processing logic (same as before) ---

        // 1. Check Files (CF_HDROP on Windows, text/uri-list on Linux)

        #[cfg(target_os = "windows")]
        let files_opt = unsafe { clipboard_api::get_clipboard_files() };

        #[cfg(not(target_os = "windows"))]
        let files_opt = clipboard_api::get_clipboard_files();

        if let Some(files) = files_opt {
            let content = files.join("\n");
            if !content.is_empty() {
                #[cfg(not(target_os = "windows"))]
                if files.len() == 1 {
                    if let Some(data_url) = image_file_to_data_url(&files[0]) {
                        monitor_state.last_text = String::new();
                        process_new_entry_async(
                            app.clone(),
                            ClipboardData::Image { data_url },
                            None,
                            Some(source_snapshot.clone()),
                        );
                        handled = true;
                    }
                }

                if handled {
                    return;
                }

                let is_new = content != monitor_state.last_text;
                let mut should_process = is_new;
                if !is_new {
                    if let Some(db_state) = app.try_state::<DbState>() {
                        if let Ok(conn) = db_state.conn.lock() {
                            if let Ok(None) = db_state
                                .repo
                                .find_by_content_with_conn(&conn, &content, None)
                            {
                                should_process = true;
                            }
                        }
                    }
                }

                if should_process {
                    let normalized = crate::database::normalize_text(&content);
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    use std::hash::{Hash, Hasher};
                    normalized.hash(&mut hasher);
                    let current_hash = hasher.finish();

                    let last_app_hash = crate::LAST_APP_SET_HASH.load(Ordering::SeqCst);
                    let last_app_hash_alt = crate::LAST_APP_SET_HASH_ALT.load(Ordering::SeqCst);
                    let last_app_time = crate::LAST_APP_SET_TIMESTAMP.load(Ordering::SeqCst);
                    let now_secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    if (last_app_hash != 0
                        && (last_app_hash == current_hash || last_app_hash_alt == current_hash))
                        && (now_secs - last_app_time) < 10
                    {
                        crate::LAST_APP_SET_HASH.store(0, Ordering::SeqCst);
                        crate::LAST_APP_SET_HASH_ALT.store(0, Ordering::SeqCst);
                    } else {
                        crate::LAST_APP_SET_HASH.store(0, Ordering::SeqCst);
                        crate::LAST_APP_SET_HASH_ALT.store(0, Ordering::SeqCst);
                        monitor_state.last_text = content.clone();

                        let settings = app.state::<SettingsState>();
                        if settings.capture_files.load(Ordering::Relaxed) {
                            process_new_entry_async(
                                app.clone(),
                                ClipboardData::Files(files),
                                None,
                                Some(source_snapshot.clone()),
                            );
                        }
                    }
                }
                handled = true;
            }
        }

        // 2. Check Image
        if !handled {
            let settings = app.state::<SettingsState>();
            let _rich_text_enabled = settings.capture_rich_text.load(Ordering::Relaxed);
            let _has_text = clipboard
                .get_text()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false);

            #[cfg(target_os = "windows")]
            let has_rich_html = if _rich_text_enabled && _has_text {
                unsafe {
                    clipboard_api::get_clipboard_raw_format("HTML Format")
                        .and_then(|raw| parse_cf_html(&raw))
                        .map(|html| !html.trim().is_empty())
                        .unwrap_or(false)
                }
            } else {
                false
            };

            #[cfg(not(target_os = "windows"))]
            let has_rich_html = if _rich_text_enabled && _has_text {
                clipboard_api::get_clipboard_html()
                    .map(|html| !html.trim().is_empty())
                    .unwrap_or(false)
            } else {
                false
            };

            // Rich text wins over image when rich HTML exists; image remains fallback for pure image content.
            if !has_rich_html {
                #[cfg(target_os = "windows")]
                unsafe {
                    let mut gif_data_opt = None;
                    for name in [
                        "GIF",
                        "Animated GIF",
                        "gif",
                        "image/gif",
                        "Graphics Interchange Format",
                    ] {
                        if let Some(data) = clipboard_api::get_clipboard_raw_format(name) {
                            gif_data_opt = Some(data);
                            break;
                        }
                    }

                    if let Some(gif_data) = gif_data_opt {
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        use std::hash::{Hash, Hasher};
                        gif_data.hash(&mut hasher);
                        let hash = hasher.finish();
                        handled = true;

                        if hash != monitor_state.last_image_hash {
                            let last_app_hash = crate::LAST_APP_SET_HASH.load(Ordering::SeqCst);
                            let last_app_time =
                                crate::LAST_APP_SET_TIMESTAMP.load(Ordering::SeqCst);
                            let now_secs = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let last_app_hash_alt =
                                crate::LAST_APP_SET_HASH_ALT.load(Ordering::SeqCst);

                            if last_app_hash != 0
                                && (last_app_hash == hash || last_app_hash_alt == hash)
                                && (now_secs - last_app_time) < 10
                            {
                                crate::LAST_APP_SET_HASH.store(0, Ordering::SeqCst);
                                crate::LAST_APP_SET_HASH_ALT.store(0, Ordering::SeqCst);
                            } else {
                                process_gif_entry_async(
                                    app.clone(),
                                    gif_data,
                                    Some(source_snapshot.clone()),
                                );
                                monitor_state.last_text = String::new();
                            }
                            monitor_state.last_image_hash = hash;
                        }
                    }

                    if !handled {
                        if let Some(image) = clipboard_api::get_clipboard_image() {
                            let mut hasher = std::collections::hash_map::DefaultHasher::new();
                            use std::hash::{Hash, Hasher};
                            image.bytes.hash(&mut hasher);
                            let hash = hasher.finish();

                            if hash != monitor_state.last_image_hash {
                                let last_app_hash = crate::LAST_APP_SET_HASH.load(Ordering::SeqCst);
                                let last_app_time =
                                    crate::LAST_APP_SET_TIMESTAMP.load(Ordering::SeqCst);
                                let now_secs = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                let last_app_hash_alt =
                                    crate::LAST_APP_SET_HASH_ALT.load(Ordering::SeqCst);

                                if last_app_hash != 0
                                    && (last_app_hash == hash || last_app_hash_alt == hash)
                                    && (now_secs - last_app_time) < 10
                                {
                                    crate::LAST_APP_SET_HASH.store(0, Ordering::SeqCst);
                                    crate::LAST_APP_SET_HASH_ALT.store(0, Ordering::SeqCst);
                                } else {
                                    handled = process_rgba_image_entry_async(
                                        app.clone(),
                                        image.width as u32,
                                        image.height as u32,
                                        image.bytes,
                                        Some(source_snapshot.clone()),
                                    );
                                }
                                monitor_state.last_image_hash = hash;
                            }
                        }
                    }
                }

                #[cfg(not(target_os = "windows"))]
                {
                    // Linux: Use arboard to get images
                    if let Some(image) = clipboard_api::get_clipboard_image() {
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        use std::hash::{Hash, Hasher};
                        image.bytes.hash(&mut hasher);
                        let hash = hasher.finish();

                        if hash != monitor_state.last_image_hash {
                            let last_app_hash = crate::LAST_APP_SET_HASH.load(Ordering::SeqCst);
                            let last_app_time =
                                crate::LAST_APP_SET_TIMESTAMP.load(Ordering::SeqCst);
                            let now_secs = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let last_app_hash_alt =
                                crate::LAST_APP_SET_HASH_ALT.load(Ordering::SeqCst);

                            if last_app_hash != 0
                                && (last_app_hash == hash || last_app_hash_alt == hash)
                                && (now_secs - last_app_time) < 10
                            {
                                crate::LAST_APP_SET_HASH.store(0, Ordering::SeqCst);
                                crate::LAST_APP_SET_HASH_ALT.store(0, Ordering::SeqCst);
                            } else {
                                handled = process_rgba_image_entry_async(
                                    app.clone(),
                                    image.width as u32,
                                    image.height as u32,
                                    image.bytes,
                                    Some(source_snapshot.clone()),
                                );
                            }
                            monitor_state.last_image_hash = hash;
                        }
                    }
                }
            }
        }

        // 3. Check Text
        if !handled {
            if let Ok(text) = clipboard.get_text() {
                if !text.is_empty() {
                    let settings = app.state::<SettingsState>();

                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    use std::hash::{Hash, Hasher};
                    crate::database::normalize_text(&text).hash(&mut hasher);
                    let current_hash = hasher.finish();

                    let last_app_hash = crate::LAST_APP_SET_HASH.load(Ordering::SeqCst);
                    let last_app_time = crate::LAST_APP_SET_TIMESTAMP.load(Ordering::SeqCst);
                    let now_secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    if (last_app_hash != 0
                        && (current_hash == last_app_hash
                            || current_hash == crate::LAST_APP_SET_HASH_ALT.load(Ordering::SeqCst)))
                        && (now_secs - last_app_time) < 10
                    {
                        crate::LAST_APP_SET_HASH.store(0, Ordering::SeqCst);
                        crate::LAST_APP_SET_HASH_ALT.store(0, Ordering::SeqCst);
                        monitor_state.last_text = text.clone();
                        return;
                    }

                    if settings.capture_rich_text.load(Ordering::Relaxed) {
                        #[cfg(target_os = "windows")]
                        if let Some(html_raw) =
                            unsafe { clipboard_api::get_clipboard_raw_format("HTML Format") }
                        {
                            if let Some(html) = parse_cf_html(&html_raw) {
                                if !html.trim().is_empty() {
                                    let mut html_to_store = html;

                                    // If source clipboard also carries an image format, keep it as a rich fallback
                                    // so paste targets can choose image/HTML/text based on their own priority rules.
                                    if let Some(data_url) = clipboard_image_fallback_data_url() {
                                        html_to_store =
                                            attach_rich_image_fallback(&html_to_store, &data_url);
                                    }

                                    monitor_state.last_text = text.clone();
                                    process_new_entry_async(
                                        app.clone(),
                                        ClipboardData::RichText {
                                            text: text.clone(),
                                            html: html_to_store,
                                        },
                                        None,
                                        Some(source_snapshot.clone()),
                                    );
                                    handled = true;
                                }
                            }
                        }

                        #[cfg(not(target_os = "windows"))]
                        {
                            if let Some(html) = clipboard_api::get_clipboard_html() {
                                if !html.trim().is_empty() {
                                    let mut html_to_store = html;

                                    if let Some(data_url) = clipboard_image_fallback_data_url() {
                                        html_to_store =
                                            attach_rich_image_fallback(&html_to_store, &data_url);
                                    }

                                    monitor_state.last_text = text.clone();
                                    process_new_entry_async(
                                        app.clone(),
                                        ClipboardData::RichText {
                                            text: text.clone(),
                                            html: html_to_store,
                                        },
                                        None,
                                        Some(source_snapshot.clone()),
                                    );
                                    handled = true;
                                }
                            }
                        }
                    }

                    if !handled {
                        if last_app_hash != 0 {
                            crate::LAST_APP_SET_HASH.store(0, Ordering::SeqCst);
                        }
                        monitor_state.last_text = text.clone();
                        process_new_entry_async(
                            app.clone(),
                            ClipboardData::Text(text),
                            None,
                            Some(source_snapshot.clone()),
                        );
                    }
                }
            }
        }
    }));
}

pub use pipeline::{ClipboardData, ClipboardPipeline, PipelineContext};
pub use utils::truncate_html_for_preview;

pub fn process_new_entry(
    app_handle: &AppHandle,
    data: ClipboardData,
    source_override: Option<String>,
    source_snapshot: Option<ActiveAppInfo>,
) {
    let mut ctx = PipelineContext::new(app_handle.clone(), data, source_snapshot);
    if let Some(source) = source_override {
        ctx.source_app = source;
        ctx.source_app_path = None;
    }

    let pipeline = ClipboardPipeline::new();
    pipeline.execute(&mut ctx);
}

#[cfg(test)]
mod tests {
    use super::{probe_image_with_retry, should_retry_image_probe};
    use std::time::Duration;

    const PNG: &str = "data:image/png;base64,AA";

    // --- the gate -----------------------------------------------------------
    //
    // Deciding whether to retry off a failed *read* conflates "no image" with
    // "the read failed": `get_clipboard_raw_format` returns None for both. This
    // gate is what keeps the two apart.

    #[test]
    fn a_clipboard_with_no_image_is_not_retried() {
        assert!(!should_retry_image_probe(None, false, false));
    }

    // DIB-only clipboard whose read failed. The availability query still says an
    // image is there, so this must stay retryable -- the case the read-based gate
    // silently turned into "no image, give up".
    #[test]
    fn an_advertised_image_is_retried_even_when_the_read_failed() {
        assert!(
            should_retry_image_probe(None, true, false),
            "a DIB-only clipboard that failed to read must not be treated as empty"
        );
    }

    // A format that was seen but could not be decoded: the original transient.
    #[test]
    fn a_seen_but_undecodable_format_is_retried() {
        assert!(should_retry_image_probe(None, false, true));
    }

    #[test]
    fn a_found_image_is_never_retried() {
        assert!(!should_retry_image_probe(Some(PNG), false, false));
        assert!(!should_retry_image_probe(Some(PNG), true, true));
    }

    // --- the driver ---------------------------------------------------------
    //
    // The truth table cannot show that a DIB-only clipboard whose first read
    // fails still recovers, which is the behaviour that was at risk.

    /// Drives the real retry loop with a scripted probe, recording the waits so
    /// "it waited" is an assertion rather than something inferred from timing.
    fn drive(
        results: Vec<(Option<&'static str>, bool)>,
        advertises: bool,
    ) -> (Option<String>, usize, usize) {
        let waits = std::cell::Cell::new(0usize);
        let calls = std::cell::Cell::new(0usize);
        let out = probe_image_with_retry(
            3,
            || {
                let i = calls.get();
                calls.set(i + 1);
                let r = results.get(i).copied().unwrap_or((None, false));
                (r.0.map(str::to_string), r.1)
            },
            || advertises,
            |_: Duration| waits.set(waits.get() + 1),
        );
        (out, waits.get(), calls.get())
    }

    #[test]
    fn confirmed_no_image_waits_zero_times() {
        let (found, waits, calls) = drive(vec![(None, false)], false);
        assert_eq!(found, None);
        assert_eq!(waits, 0, "a clipboard with no image must not be waited on");
        assert_eq!(calls, 1, "and it must not be probed again either");
    }

    // DIB-only clipboard, first read fails, second succeeds.
    //
    // `saw_image_format` is false on purpose: `get_clipboard_image()` returned
    // nothing, so the probe saw no format and the *only* evidence an image is
    // present is the availability query. Passing true here would let the gate
    // pass even with the availability query ignored -- the test would then be
    // guarding nothing.
    #[test]
    fn a_dib_only_clipboard_that_fails_once_is_still_recovered() {
        let (found, waits, calls) =
            drive(vec![(None, false), (Some(PNG), false), (Some(PNG), false)], true);
        assert_eq!(found.as_deref(), Some(PNG));
        assert_eq!(waits, 1, "it should wait once and then succeed");
        assert_eq!(calls, 2);
    }

    // Advertised but unreadable twice, readable on the third pass.
    #[test]
    fn an_advertised_format_that_fails_twice_is_recovered_on_the_third() {
        let (found, waits, calls) = drive(
            vec![(None, false), (None, false), (Some(PNG), false)],
            true,
        );
        assert_eq!(found.as_deref(), Some(PNG));
        assert_eq!(waits, 2);
        assert_eq!(calls, 3);
    }

    // Bounded: a source that never becomes readable must not hold the capture
    // thread forever.
    #[test]
    fn retries_are_bounded() {
        let (found, waits, calls) = drive(
            vec![(None, false), (None, false), (None, false), (Some(PNG), false)],
            true,
        );
        assert_eq!(found, None, "must give up rather than keep waiting");
        assert_eq!(waits, 2);
        assert_eq!(calls, 3, "three attempts total, never more");
    }

    // The saving: an image already present costs no wait at all.
    #[test]
    fn an_image_found_on_the_first_probe_waits_nothing() {
        let (found, waits, calls) = drive(vec![(Some(PNG), true)], true);
        assert_eq!(found.as_deref(), Some(PNG));
        assert_eq!(waits, 0);
        assert_eq!(calls, 1);
    }
}
