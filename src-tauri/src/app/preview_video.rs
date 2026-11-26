//! File-backed media available only to the pinned WebView that opened it.
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, Mutex},
};

use dzc_host::wire::{DesktopEntryParams, LocalVideoSource, Method};
use tauri::{
    http::{header, Method as HttpMethod, Request, Response, StatusCode},
    AppHandle, Manager, WebviewWindow,
};

use super::ipc_host::DesktopHost;

#[cfg(target_os = "linux")]
mod loopback;

const SCHEME: &str = "dzcvideo";
const RANGE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_UNRANGED_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Default)]
pub struct Videos {
    active: Mutex<HashMap<String, Arc<Video>>>,
    #[cfg(target_os = "linux")]
    port: tokio::sync::OnceCell<u16>,
}

#[derive(serde::Serialize)]
pub struct PlaybackSource {
    token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

struct Video {
    token: String,
    file: Mutex<File>,
    length: u64,
    mime: &'static str,
    instance: String,
    epoch: u64,
}

fn media_type(path: &Path) -> Option<&'static str> {
    Some(match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "wmv" => "video/x-ms-wmv",
        _ => return None,
    })
}

#[tauri::command]
pub async fn open_pinned_video(
    window: WebviewWindow,
    id: i64,
    expected_epoch: u64,
    expected_host_instance_id: Option<String>,
) -> Result<PlaybackSource, String> {
    if !window
        .label()
        .strip_prefix("pinned-preview-")
        .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok())
    {
        return Err("VideoPreviewUnavailable".into());
    }
    let bridge = window.state::<DesktopHost>();
    bridge.check_instance(id, expected_host_instance_id.as_deref())?;
    let instance = bridge.instance()?;
    if expected_host_instance_id.as_ref().is_some_and(|expected| *expected != instance) {
        return Err("ClipboardHostInstanceChanged".into());
    }
    let source: LocalVideoSource = bridge
        .call(Method::DesktopHistoryLocalVideo, DesktopEntryParams {
            id, expected_epoch, service_instance_id: instance.clone(), reveal: false,
            thumbnail_dimension: None, include_video_frame: None, omit_ocr_text: None,
        }, "LocalVideoSource")
        .await?;
    if source.state.service_instance_id != instance || source.state.privacy_epoch != expected_epoch {
        return Err("PrivacyEpochChanged".into());
    }
    let path = Path::new(&source.path);
    let mime = media_type(path).ok_or("VideoPreviewUnavailable")?;
    let file = dzc_platform::local_file::open_preview_file(path).ok_or("VideoPreviewUnavailable")?;
    let length = file.metadata().map_err(|_| "VideoPreviewUnavailable")?.len();
    if length == 0 {
        return Err("VideoPreviewUnavailable".into());
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    #[cfg(target_os = "linux")]
    let url = Some(loopback::url(window.app_handle(), &token).await?);
    #[cfg(not(target_os = "linux"))]
    let url = None;
    let videos = window.state::<Videos>();
    let mut active = videos.active.lock().map_err(|_| "VideoPreviewUnavailable")?;
    bridge.check_queue_state(&instance, expected_epoch)?;
    if window.get_webview_window(window.label()).is_none() {
        return Err("VideoPreviewUnavailable".into());
    }
    active.insert(window.label().to_owned(), Arc::new(Video {
        token: token.clone(), file: Mutex::new(file), length, mime, instance, epoch: expected_epoch,
    }));
    Ok(PlaybackSource { token, url })
}

#[tauri::command]
pub fn close_pinned_video(window: WebviewWindow, token: String) {
    let videos = window.state::<Videos>();
    if let Ok(mut active) = videos.active.lock() {
        if active.get(window.label()).is_some_and(|video| video.token == token) {
            active.remove(window.label());
        }
    };
}

pub fn close_window(app: &AppHandle, label: &str) {
    let videos = app.state::<Videos>();
    if let Ok(mut active) = videos.active.lock() {
        active.remove(label);
    };
}

fn bounds(value: &str, length: u64) -> Option<(u64, u64)> {
    range_bounds(value, length, RANGE_BYTES)
}

fn range_bounds(value: &str, length: u64, limit: u64) -> Option<(u64, u64)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    let (start, end) = if start.is_empty() {
        let count = end.parse::<u64>().ok()?.min(length).min(limit);
        if count == 0 {
            return None;
        }
        (length - count, length - 1)
    } else {
        let start = start.parse::<u64>().ok()?;
        let end = if end.is_empty() { length - 1 } else { end.parse().ok()? };
        (start, end.min(length - 1).min(start.saturating_add(limit - 1)))
    };
    (start <= end).then_some((start, end))
}

fn response(app: &AppHandle, label: &str, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let empty = |status| Response::builder().status(status).header(header::CACHE_CONTROL, "no-store")
        .body(Vec::new()).expect("static response headers");
    let video = {
        let videos = app.state::<Videos>();
        let Ok(active) = videos.active.lock() else { return empty(StatusCode::NOT_FOUND); };
        let Some(video) = active.get(label) else { return empty(StatusCode::NOT_FOUND); };
        if request.uri().path().strip_prefix('/') != Some(video.token.as_str()) {
            return empty(StatusCode::NOT_FOUND);
        }
        video.clone()
    };
    let bridge = app.state::<DesktopHost>();
    if bridge.check_queue_state(&video.instance, video.epoch).is_err() {
        return empty(StatusCode::GONE);
    }
    let head = request.method() == HttpMethod::HEAD;
    if !head && request.method() != HttpMethod::GET {
        return empty(StatusCode::METHOD_NOT_ALLOWED);
    }
    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, video.mime)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCEPT_RANGES, "bytes");
    let (start, end) = if let Some(range) = request.headers().get(header::RANGE) {
        let Some((start, end)) = range.to_str().ok().and_then(|s| bounds(s, video.length)) else {
            return builder.status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{}", video.length))
                .body(Vec::new()).expect("known range headers");
        };
        builder = builder.status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{}", video.length));
        (start, end)
    } else if head || video.length <= MAX_UNRANGED_BYTES {
        (0, video.length - 1)
    } else {
        return empty(StatusCode::RANGE_NOT_SATISFIABLE);
    };
    let count = end - start + 1;
    let mut bytes = Vec::new();
    if !head {
        let Ok(mut file) = video.file.lock() else { return empty(StatusCode::GONE); };
        bytes.resize(count as usize, 0);
        if file.seek(SeekFrom::Start(start)).and_then(|_| file.read_exact(&mut bytes)).is_err()
            || bridge.check_queue_state(&video.instance, video.epoch).is_err()
        {
            return empty(StatusCode::GONE);
        }
        let videos = app.state::<Videos>();
        if !videos.active.lock().is_ok_and(|active| active.get(label).is_some_and(|current| Arc::ptr_eq(current, &video))) {
            return empty(StatusCode::GONE);
        }
    }
    builder.header(header::CONTENT_LENGTH, count).body(bytes).expect("known media headers")
}

pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.manage(Videos::default()).register_asynchronous_uri_scheme_protocol(SCHEME, |context, request, responder| {
        let app = context.app_handle().clone();
        let label = context.webview_label().to_owned();
        tauri::async_runtime::spawn_blocking(move || responder.respond(response(&app, &label, request)));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_ranges_are_seekable_and_bounded() {
        assert_eq!(bounds("bytes=0-1", 100), Some((0, 1)));
        assert_eq!(bounds("bytes=80-", 100), Some((80, 99)));
        assert_eq!(bounds("bytes=-10", 100), Some((90, 99)));
        assert_eq!(bounds("bytes=100-", 100), None);
        assert_eq!(bounds("bytes=20-10", 100), None);
        assert_eq!(bounds("bytes=1-2,3-4", 100), None);
        assert_eq!(bounds("bytes=12-", RANGE_BYTES * 4), Some((12, RANGE_BYTES + 11)));
        assert_eq!(range_bounds("bytes=12-", RANGE_BYTES * 4, u64::MAX),
            Some((12, RANGE_BYTES * 4 - 1)));
    }
}
