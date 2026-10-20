use base64::{engine::general_purpose::STANDARD, Engine};
use dzc_core::capture::{
    CaptureData, CaptureFailure, CaptureReadMetadata, CaptureReadRequest, CaptureReader,
    CaptureSkip, CaptureStage,
};
use std::hash::{Hash, Hasher};

#[derive(Default)]
pub struct NativeCaptureReader {
    metadata: CaptureReadMetadata,
    empty: Option<CaptureSkip>,
    #[cfg(target_os = "linux")]
    wayland_selection: Option<crate::wayland::SelectionSnapshot>,
}

fn fingerprint(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn files() -> Option<Vec<String>> {
    #[cfg(target_os = "windows")]
    unsafe {
        if windows::Win32::System::DataExchange::IsClipboardFormatAvailable(15).is_err() {
            return None;
        }
        super::windows_clipboard::get_clipboard_files()
    }
    #[cfg(target_os = "linux")]
    {
        super::x11_clipboard::get_clipboard_files()
    }
}

struct Pixels {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

fn pixels() -> Option<Pixels> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;
        if IsClipboardFormatAvailable(8).is_err() && IsClipboardFormatAvailable(17).is_err() {
            return None;
        }
    }
    #[cfg(target_os = "windows")]
    let image = unsafe { super::windows_clipboard::get_clipboard_image()? };
    #[cfg(target_os = "linux")]
    let image = arboard::Clipboard::new().ok()?.get_image().ok()?;
    Some(Pixels {
        width: image.width as u32,
        height: image.height as u32,
        #[cfg(target_os = "windows")]
        bytes: image.bytes,
        #[cfg(target_os = "linux")]
        bytes: image.bytes.into_owned(),
    })
}

fn png_data_url(image: Pixels) -> Option<String> {
    let image = image::RgbaImage::from_raw(image.width, image.height, image.bytes)?;
    let mut output = std::io::Cursor::new(Vec::new());
    image.write_to(&mut output, image::ImageFormat::Png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(output.into_inner())
    ))
}

fn html(clipboard: &mut arboard::Clipboard) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let _ = clipboard;
        unsafe { super::windows_clipboard::get_clipboard_raw_format("HTML Format") }
            .and_then(|raw| super::parse_cf_html(&raw))
            .filter(|html| !html.trim().is_empty())
    }
    #[cfg(target_os = "linux")]
    {
        clipboard
            .get()
            .html()
            .ok()
            .filter(|html| !html.trim().is_empty())
    }
}

#[cfg(target_os = "windows")]
fn gif() -> Option<Vec<u8>> {
    for name in [
        "GIF",
        "Animated GIF",
        "gif",
        "image/gif",
        "Graphics Interchange Format",
    ] {
        if let Some(bytes) = unsafe { super::windows_clipboard::get_clipboard_raw_format(name) } {
            return Some(bytes);
        }
    }
    None
}

#[cfg(any(target_os = "windows", test))]
const IMAGE_PROBE_ATTEMPTS: usize = 3;
#[cfg(any(target_os = "windows", test))]
const IMAGE_PROBE_RETRY_DELAY_MS: u64 = 35;

#[cfg(any(target_os = "windows", test))]
fn should_retry_image_probe(found: Option<&str>, advertises: bool, saw_format: bool) -> bool {
    found.is_none() && (advertises || saw_format)
}

#[cfg(any(target_os = "windows", test))]
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
    let (found, saw_format) = probe();
    if found.is_some() {
        return found;
    }
    if !should_retry_image_probe(None, advertises(), saw_format) {
        return None;
    }
    for _ in 1..attempts {
        wait(std::time::Duration::from_millis(IMAGE_PROBE_RETRY_DELAY_MS));
        if let Some(found) = probe().0 {
            return Some(found);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn image_fallback() -> Option<String> {
    let probe = || {
        let mut saw_format = false;
        for name in ["PNG", "image/png", "JFIF", "JPEG", "image/jpeg"] {
            if let Some(raw) = unsafe { super::windows_clipboard::get_clipboard_raw_format(name) } {
                saw_format = true;
                let bytes = if raw.len() > 8 && raw.starts_with(b"\x89PNG\r\n\x1a\n") {
                    Some(raw)
                } else {
                    image::load_from_memory(&raw).ok().and_then(|image| {
                        let mut output = std::io::Cursor::new(Vec::new());
                        image.write_to(&mut output, image::ImageFormat::Png).ok()?;
                        Some(output.into_inner())
                    })
                };
                if let Some(bytes) = bytes {
                    return (
                        Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes))),
                        true,
                    );
                }
            }
        }
        if let Some(image) = pixels() {
            saw_format = true;
            if let Some(url) = png_data_url(image) {
                return (Some(url), true);
            }
        }
        (None, saw_format)
    };
    probe_image_with_retry(
        IMAGE_PROBE_ATTEMPTS,
        probe,
        super::windows_clipboard::clipboard_advertises_image,
        std::thread::sleep,
    )
}

#[cfg(target_os = "linux")]
fn image_fallback() -> Option<String> {
    png_data_url(pixels()?)
}

impl NativeCaptureReader {
    #[cfg(target_os = "linux")]
    fn read_wayland(
        &mut self,
        request: CaptureReadRequest,
    ) -> Result<Option<CaptureData>, CaptureFailure> {
        use crate::wayland::{WaylandClipboardBackend, WaylandError, MAX_TRANSFER_BYTES};
        let failure = |error: WaylandError| CaptureFailure {
            stage: CaptureStage::Read,
            code: error.to_string(),
        };
        let selection = match self
            .wayland_selection
            .take()
            .map(Ok)
            .unwrap_or_else(WaylandClipboardBackend::snapshot)
        {
            Ok(value) => value,
            Err(WaylandError::EmptySelection) => return Ok(None),
            Err(error) => return Err(failure(error)),
        };
        if !selection.is_current() {
            return Err(failure(WaylandError::SelectionChanged));
        }
        // Explicit source exclusions protect capture even before the optional
        // Core preflight adapter is attached. No body is requested on this path.
        if selection.password_hint().map_err(failure)? == Some(true) {
            return Err(CaptureFailure {
                stage: CaptureStage::Read,
                code: "WaylandExplicitPrivacyExclusion".into(),
            });
        }
        let has = |mime: &str| selection.mime_types().iter().any(|value| value == mime);
        let remaining = std::cell::Cell::new(MAX_TRANSFER_BYTES);
        let receive = |mime: &str, budget: usize| {
            let bytes = selection
                .receive(mime, budget.min(remaining.get()))
                .map_err(failure)?;
            remaining.set(remaining.get() - bytes.len());
            Ok::<_, CaptureFailure>(bytes)
        };
        let local_files = if has("text/uri-list") {
            let bytes = receive("text/uri-list", 1024 * 1024)?;
            super::x11_clipboard::parse_uri_list_payload(&bytes)
        } else { None };
        let data = if let Some(files) = local_files {
            if !request.files {
                self.empty = Some(CaptureSkip::FilesDisabled);
                return Ok(None);
            }
            let hash = fingerprint(files.join("\n"));
            self.metadata = CaptureReadMetadata {
                fingerprint: Some(hash),
                echo_hash: Some(hash),
                files: true,
                ..Default::default()
            };
            CaptureData::Files(files)
        } else {
            let text_mime = [
                "text/plain;charset=utf-8",
                "text/plain;charset=UTF-8",
                "UTF8_STRING",
                "text/plain",
            ]
            .into_iter()
            .find(|mime| has(mime));
            let image_mime = [
                "image/png",
                "image/jpeg",
                "image/gif",
                "image/webp",
                "image/bmp",
            ]
            .into_iter()
            .find(|mime| has(mime));
            let read_text = |mime: &str, budget| {
                receive(mime, budget).and_then(|bytes| {
                    String::from_utf8(bytes).map_err(|_| CaptureFailure {
                        stage: CaptureStage::Read,
                        code: "WaylandClipboardInvalidUtf8".into(),
                    })
                })
            };
            let mut text = None;
            let html = if request.rich_text && has("text/html") {
                if let Some(mime) = text_mime {
                    text = Some(read_text(mime, MAX_TRANSFER_BYTES)?);
                    if text.as_ref().is_some_and(|value| !value.is_empty()) {
                        Some(read_text("text/html", 16 * 1024 * 1024)?)
                            .filter(|value| !value.trim().is_empty())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(mut html) = html {
                if let Some(mime) = image_mime {
                    let image = receive(mime, 16 * 1024 * 1024)?;
                    html.push_str(&format!(
                        "\n<!--TIEZ_RICH_IMAGE:data:{mime};base64,{}-->",
                        STANDARD.encode(image)
                    ));
                }
                let text = text.expect("rich HTML requires nonempty raw text");
                self.metadata = CaptureReadMetadata {
                    fingerprint: Some(fingerprint((&text, &html))),
                    echo_hash: Some(fingerprint(&text)),
                    ..Default::default()
                };
                CaptureData::RichText { text, html }
            } else if let Some(mime) = image_mime {
                let bytes = receive(mime, MAX_TRANSFER_BYTES)?;
                let hash = fingerprint(&bytes);
                self.metadata = CaptureReadMetadata {
                    fingerprint: Some(hash),
                    echo_hash: Some(hash),
                    image: true,
                    ..Default::default()
                };
                CaptureData::EncodedImage {
                    bytes,
                    mime: mime.to_owned(),
                }
            } else if let Some(mime) = text_mime {
                let text = match text {
                    Some(value) => value,
                    None => read_text(mime, MAX_TRANSFER_BYTES)?,
                };
                if text.is_empty() {
                    return Ok(None);
                }
                let hash = fingerprint(&text);
                self.metadata = CaptureReadMetadata {
                    fingerprint: Some(hash),
                    echo_hash: Some(hash),
                    ..Default::default()
                };
                CaptureData::Text(text)
            } else {
                return Ok(None);
            }
        };
        if !selection.is_current() {
            return Err(failure(WaylandError::SelectionChanged));
        }
        Ok(Some(data))
    }
    fn image(&mut self, image: Pixels) -> CaptureData {
        let hash = fingerprint(image.bytes.as_slice());
        self.metadata = CaptureReadMetadata {
            fingerprint: Some(hash),
            echo_hash: Some(hash),
            image: true,
            files: false,
            image_content_hash: None,
            ..CaptureReadMetadata::default()
        };
        CaptureData::RgbaImage {
            width: image.width,
            height: image.height,
            bytes: image.bytes,
        }
    }
}

impl CaptureReader for NativeCaptureReader {
    #[cfg(target_os = "windows")]
    fn preflight_privacy(&mut self) -> Result<dzc_core::capture::PrivacyPreflight, CaptureFailure> {
        super::windows_privacy::preflight()
    }
    #[cfg(target_os = "windows")]
    fn selection_id(&self) -> Option<u64> { super::windows_privacy::selection_id() }
    fn read(&mut self, request: CaptureReadRequest) -> Result<Option<CaptureData>, CaptureFailure> {
        self.metadata = CaptureReadMetadata::default();
        self.empty = None;
        #[cfg(target_os = "linux")]
        if crate::wayland::requested() {
            return self.read_wayland(request);
        }
        #[cfg(target_os = "linux")]
        crate::runtime_display::validate_x11_display().map_err(|error| CaptureFailure {
            stage: CaptureStage::Read,
            code: error.to_string(),
        })?;
        #[cfg(target_os = "windows")]
        if !request.files
            && unsafe {
                windows::Win32::System::DataExchange::IsClipboardFormatAvailable(15).is_ok()
            }
        {
            self.empty = Some(CaptureSkip::FilesDisabled);
            return Ok(None);
        }
        #[cfg(target_os = "windows")]
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(files) = files().filter(|files| !files.is_empty()) {
            #[cfg(target_os = "linux")]
            if files.len() == 1 {
                let lower = files[0].to_ascii_lowercase();
                if [".png", ".jpg", ".jpeg", ".bmp", ".webp", ".gif"]
                    .iter()
                    .any(|suffix| lower.ends_with(suffix))
                {
                    if let Ok(image) = image::open(&files[0]) {
                        let image = image::DynamicImage::ImageRgba8(image.into_rgba8());
                        let content_hash = dzc_core::capture::thumbnail_hash(&image);
                        let image = image.into_rgba8();
                        let (width, height) = image.dimensions();
                        let data = self.image(Pixels {
                            width,
                            height,
                            bytes: image.into_raw(),
                        });
                        self.metadata.files = true;
                        self.metadata.fingerprint = Some(fingerprint((
                            files.as_slice(),
                            width,
                            height,
                            self.metadata.echo_hash,
                        )));
                        self.metadata.image_content_hash = Some(content_hash);
                        return Ok(Some(data));
                    }
                }
            }
            if !request.files {
                self.empty = Some(CaptureSkip::FilesDisabled);
                return Ok(None);
            }
            let hash = fingerprint(files.join("\n"));
            self.metadata = CaptureReadMetadata {
                fingerprint: Some(hash),
                echo_hash: Some(hash),
                image: false,
                files: true,
                image_content_hash: None,
                ..CaptureReadMetadata::default()
            };
            return Ok(Some(CaptureData::Files(files)));
        }
        let mut text = None;
        if request.rich_text {
            let mut clipboard = arboard::Clipboard::new().map_err(|_| CaptureFailure {
                stage: CaptureStage::Read,
                code: "ClipboardReadUnavailable".into(),
            })?;
            text = clipboard.get_text().ok().filter(|text| !text.is_empty());
            if text.as_ref().is_some_and(|text| !text.is_empty()) {
                if let Some(mut html) = html(&mut clipboard) {
                    if let Some(fallback) = image_fallback() {
                        html = format!("{}\n<!--TIEZ_RICH_IMAGE:{fallback}-->", html.trim_end());
                    }
                    let text = text.expect("text was checked above");
                    self.metadata = CaptureReadMetadata {
                        fingerprint: Some(fingerprint((&text, &html))),
                        echo_hash: Some(fingerprint(&text)),
                        image: false,
                        files: false,
                        image_content_hash: None,
                        ..CaptureReadMetadata::default()
                    };
                    return Ok(Some(CaptureData::RichText { text, html }));
                }
            }
        }
        #[cfg(target_os = "windows")]
        if let Some(bytes) = gif() {
            let hash = fingerprint(&bytes);
            self.metadata = CaptureReadMetadata {
                fingerprint: Some(hash),
                echo_hash: Some(hash),
                image: true,
                files: false,
                image_content_hash: None,
                ..CaptureReadMetadata::default()
            };
            return Ok(Some(CaptureData::EncodedImage {
                bytes,
                mime: "image/gif".into(),
            }));
        }
        if let Some(image) = pixels() {
            return Ok(Some(self.image(image)));
        }
        if !request.rich_text {
            let mut clipboard = arboard::Clipboard::new().map_err(|_| CaptureFailure {
                stage: CaptureStage::Read,
                code: "ClipboardReadUnavailable".into(),
            })?;
            text = clipboard.get_text().ok().filter(|text| !text.is_empty());
        }
        if let Some(text) = text {
            let hash = fingerprint(&text);
            self.metadata = CaptureReadMetadata {
                fingerprint: Some(hash),
                echo_hash: Some(hash),
                image: false,
                files: false,
                image_content_hash: None,
                ..CaptureReadMetadata::default()
            };
            return Ok(Some(CaptureData::Text(text)));
        }
        Ok(None)
    }

    fn metadata(&self) -> CaptureReadMetadata {
        self.metadata
    }
    fn empty_reason(&self) -> CaptureSkip {
        self.empty.unwrap_or(CaptureSkip::Empty)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        probe_image_with_retry, should_retry_image_probe, IMAGE_PROBE_ATTEMPTS,
        IMAGE_PROBE_RETRY_DELAY_MS,
    };
    use std::time::Duration;

    const PNG: &str = "data:image/png;base64,AA";

    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_x11_reader_keeps_text_html_and_file_priority() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        use super::{CaptureData, CaptureReadRequest, CaptureReader, NativeCaptureReader};
        let text = "  native capture\r\n\tsecond line\r\n";
        super::super::x11_clipboard::set_clipboard_text_and_html(text.into(), None).unwrap();
        let mut reader = NativeCaptureReader::default();
        assert!(
            matches!(reader.read(CaptureReadRequest { files: false, rich_text: false }).unwrap(), Some(CaptureData::Text(value)) if value == text)
        );
        let html = "<pre>  native capture\r\nsecond</pre>";
        super::super::x11_clipboard::set_clipboard_text_and_html(text.into(), Some(html.into()))
            .unwrap();
        assert!(
            matches!(reader.read(CaptureReadRequest { files: false, rich_text: true }).unwrap(), Some(CaptureData::RichText { text: value, html: markup }) if value == text && markup == html)
        );
        let whitespace = " \t\r\n";
        let whitespace_html = "<p><br></p>";
        super::super::x11_clipboard::set_clipboard_text_and_html(
            whitespace.into(),
            Some(whitespace_html.into()),
        )
        .unwrap();
        assert!(
            matches!(reader.read(CaptureReadRequest { files: false, rich_text: true }).unwrap(), Some(CaptureData::RichText { text: value, html: markup }) if value == whitespace && markup == whitespace_html)
        );
        let path =
            std::env::temp_dir().join(format!("dezirclip-reader-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"fixture").unwrap();
        super::super::x11_clipboard::set_clipboard_files(vec![path.to_string_lossy().into_owned()])
            .unwrap();
        assert!(
            matches!(reader.read(CaptureReadRequest { files: true, rich_text: true }).unwrap(), Some(CaptureData::Files(files)) if files == vec![path.to_string_lossy().into_owned()])
        );
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_x11_file_images_recapture_after_delete_text_and_another_image() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        use super::{CaptureData, CaptureReadRequest, CaptureReader, NativeCaptureReader};
        use base64::{engine::general_purpose::STANDARD, Engine};
        use dzc_core::{
            api::{HistoryQuery, HistoryService},
            capture::*,
        };
        use std::{collections::VecDeque, sync::Mutex};
        let directory = std::env::temp_dir().join(format!(
            "dezirclip-native-file-recapture-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory).unwrap();
        let database_path = directory.join("clipboard.db");
        let history = HistoryService::open(&database_path).unwrap();
        let first_file = directory.join("a.png");
        let second_file = directory.join("b.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .save(&first_file)
            .unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([7, 8, 9, 255]))
            .save(&second_file)
            .unwrap();
        let store = history.capture_store(&directory);
        let session = Mutex::new(VecDeque::new());
        let mut notifications = CaptureNotificationState::default();
        let process_mode = |notifications: &mut CaptureNotificationState, timestamp, persistent| {
            let context = CaptureContext {
                source: CaptureSource {
                    app_name: "File manager".into(),
                    process_path: None,
                },
                timestamp,
                settings: CaptureSettings {
                    persistent,
                    deduplicate: true,
                    ..CaptureSettings::default()
                },
                session: &session,
                paused: false,
                sequential_echo: None,
            };
            let mut reader = NativeCaptureReader::default();
            let data = reader
                .read(CaptureReadRequest {
                    files: false,
                    rich_text: true,
                })
                .unwrap()
                .unwrap();
            let metadata = reader.metadata();
            if matches!(&data, CaptureData::RgbaImage { .. }) {
                assert!(metadata.image && metadata.files && metadata.image_content_hash.is_some());
            }
            let frame = CaptureFrame { data, metadata };
            notifications.set_persistence_mode(persistent);
            let exists = notifications.repeats_file(metadata)
                && CaptureEngine::file_history_exists(&context, &frame, &store).unwrap();
            if let Err(reason) = notifications.inspect(
                metadata,
                timestamp as u64,
                OwnClipboardCopy::default(),
                exists,
            ) {
                return CaptureOutcome::Skipped(reason);
            }
            let outcome = CaptureEngine::capture_frame(&context, frame, &store, &mut ());
            if let (CaptureOutcome::Stored(capture), Some(hash)) =
                (&outcome, metadata.image_content_hash)
            {
                assert_eq!(
                    Some(thumbnail_hash(
                        &image::load_from_memory(
                            &STANDARD
                                .decode(capture.entry.content.split_once(',').unwrap().1)
                                .unwrap()
                        )
                        .unwrap()
                    )),
                    Some(hash)
                );
                assert!(store.contains_image_hash(hash).unwrap());
            }
            outcome
        };
        let process = |notifications: &mut CaptureNotificationState, timestamp| {
            process_mode(notifications, timestamp, true)
        };
        let copy_file = |path: &std::path::Path| {
            super::super::x11_clipboard::set_clipboard_files(vec![path
                .to_string_lossy()
                .into_owned()])
            .unwrap()
        };
        copy_file(&first_file);
        let CaptureOutcome::Stored(first) = process(&mut notifications, 1000) else {
            panic!("first native file image should store");
        };
        copy_file(&first_file);
        assert!(matches!(
            process(&mut notifications, 1600),
            CaptureOutcome::Skipped(CaptureSkip::DuplicateNotification)
        ));
        let external = HistoryService::open(&database_path).unwrap();
        external.delete(first.entry.id, None).unwrap();
        copy_file(&first_file);
        let CaptureOutcome::Stored(restored_inside_window) = process(&mut notifications, 1650)
        else {
            panic!("deleted native file image should recapture within debounce window");
        };
        copy_file(&first_file);
        assert!(matches!(
            process(&mut notifications, 2300),
            CaptureOutcome::Skipped(CaptureSkip::DuplicateNotification)
        ));
        external
            .delete(restored_inside_window.entry.id, None)
            .unwrap();
        copy_file(&first_file);
        let CaptureOutcome::Stored(restored) = process(&mut notifications, 2900) else {
            panic!("deleted native file image should recapture after debounce window");
        };
        super::super::x11_clipboard::set_clipboard_text_and_html("intervening text".into(), None)
            .unwrap();
        assert!(matches!(
            process(&mut notifications, 3000),
            CaptureOutcome::Stored(_)
        ));
        copy_file(&first_file);
        let CaptureOutcome::Stored(after_text) = process(&mut notifications, 3050) else {
            panic!("file image after text should recapture");
        };
        assert_eq!(restored.entry.id, after_text.entry.id);
        assert_eq!(after_text.entry.timestamp, 3050);
        copy_file(&second_file);
        let CaptureOutcome::Stored(second) = process(&mut notifications, 3100) else {
            panic!("different file image should store");
        };
        assert_ne!(second.entry.id, after_text.entry.id);
        copy_file(&first_file);
        let CaptureOutcome::Stored(after_other_image) = process(&mut notifications, 3150) else {
            panic!("file image after another image should recapture");
        };
        assert_eq!(after_other_image.entry.id, restored.entry.id);
        assert_eq!(after_other_image.entry.timestamp, 3150);
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 11, 12, 255]))
            .save(&first_file)
            .unwrap();
        copy_file(&first_file);
        let CaptureOutcome::Stored(changed_bytes) = process(&mut notifications, 3200) else {
            panic!("changed file bytes at the same path should capture");
        };
        assert_ne!(changed_bytes.entry.id, restored.entry.id);
        image::RgbaImage::from_pixel(2, 2, image::Rgba([7, 8, 9, 255]))
            .save(&first_file)
            .unwrap();
        copy_file(&first_file);
        let CaptureOutcome::Stored(changed_to_existing) = process(&mut notifications, 3250) else {
            panic!(
                "changed file bytes should update even when another row already has those pixels"
            );
        };
        assert_eq!(changed_to_existing.entry.id, second.entry.id);
        assert_eq!(changed_to_existing.entry.timestamp, 3250);
        copy_file(&first_file);
        let CaptureOutcome::Stored(session_capture) = process_mode(&mut notifications, 3300, false)
        else {
            panic!("switching from persistent image to session must capture");
        };
        assert!(session_capture.entry.id < 0 && !session_capture.persistent);
        assert_eq!(session.lock().unwrap().len(), 1);
        copy_file(&first_file);
        let CaptureOutcome::Stored(session_repeat) = process_mode(&mut notifications, 3900, false)
        else {
            panic!("session repeats must use the session deduplicator");
        };
        assert_eq!(session_repeat.entry.id, session_capture.entry.id);
        session
            .lock()
            .unwrap()
            .retain(|entry| entry.id != session_capture.entry.id);
        copy_file(&first_file);
        let CaptureOutcome::Stored(after_session_delete) =
            process_mode(&mut notifications, 3950, false)
        else {
            panic!("deleting a session image must allow recapture despite an old durable row");
        };
        assert!(after_session_delete.entry.id < 0 && !after_session_delete.persistent);
        assert_eq!(session.lock().unwrap().len(), 1);
        copy_file(&first_file);
        let CaptureOutcome::Stored(back_to_persistent) = process(&mut notifications, 4000) else {
            panic!("returning to persistent capture must reach the durable deduplicator");
        };
        assert_eq!(back_to_persistent.entry.id, second.entry.id);
        assert_eq!(back_to_persistent.entry.timestamp, 4000);
        assert_eq!(
            history
                .query(&HistoryQuery {
                    kind: Some("image"),
                    limit: 100,
                    ..HistoryQuery::default()
                })
                .unwrap()
                .len(),
            3
        );
        drop(external);
        drop(store);
        drop(history);
        std::fs::remove_dir_all(directory).unwrap();
    }

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
    ///
    /// The budget comes from the production constant on purpose. An earlier
    /// version passed 3 here and 2 in production, so "recovers on the third
    /// pass" was green while the shipped path never made a third pass -- the
    /// test was guarding a schedule production did not use.
    fn drive(
        results: Vec<(Option<&'static str>, bool)>,
        advertises: bool,
    ) -> (Option<String>, usize, usize) {
        let waits = std::cell::Cell::new(0usize);
        let calls = std::cell::Cell::new(0usize);
        let out = probe_image_with_retry(
            IMAGE_PROBE_ATTEMPTS,
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

    // The boundary, asserted rather than left implied: a format that appears
    // only *after* the first check is not waited for. This is a deliberate rule,
    // not a gap that happens to be untested -- see the note on the gate.
    #[test]
    fn a_format_added_after_the_first_check_is_not_awaited() {
        let (found, waits, calls) = drive(vec![(None, false), (Some(PNG), false)], false);
        assert_eq!(found, None, "absence on the first pass ends the search");
        assert_eq!(waits, 0);
        assert_eq!(calls, 1);
    }

    // The budget is the pre-refactor one, kept as a named constant precisely so
    // it cannot drift away from what the tests exercise.
    #[test]
    fn the_budget_matches_what_the_old_loop_gave() {
        assert_eq!(
            IMAGE_PROBE_ATTEMPTS, 3,
            "the old loop probed at 0/35/70 ms; fewer attempts drops a case the \
             old code recovered from"
        );
        assert_eq!(IMAGE_PROBE_RETRY_DELAY_MS, 35);
    }

    // Sharing the constant stops the two drifting apart, but only if the
    // production call actually reads it -- a literal there would leave the tests
    // exercising a schedule production does not use, which is exactly what
    // happened before. This pins that the call site passes the constant.
    //
    // It is a source assertion, so it proves the call site names the constant;
    // it does not prove anything about Windows, and the driver tests above carry
    // that.
    #[test]
    fn the_production_call_uses_the_shared_budget() {
        let source = include_str!("reader.rs");
        let call = source
            .split("probe_image_with_retry(")
            .nth(1)
            .expect("the production call site must exist");
        let first_arg = call.split(',').next().expect("it must pass an argument");
        assert_eq!(
            first_arg.trim(),
            "IMAGE_PROBE_ATTEMPTS",
            "production must pass the shared budget, not a literal -- a literal \
             here is invisible to the driver tests"
        );
    }

    // ...and this is the case that budget exists for: readable only on the third
    // probe. With two attempts the shipped path returned None here.
    #[test]
    fn an_image_readable_only_on_the_third_probe_is_still_recovered() {
        let (found, waits, calls) =
            drive(vec![(None, false), (None, false), (Some(PNG), false)], true);
        assert_eq!(found.as_deref(), Some(PNG));
        assert_eq!(waits, 2, "it waits for the whole budget before succeeding");
        assert_eq!(calls, IMAGE_PROBE_ATTEMPTS);
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
        let (found, waits, calls) = drive(
            vec![(None, false), (Some(PNG), false), (Some(PNG), false)],
            true,
        );
        assert_eq!(found.as_deref(), Some(PNG));
        assert_eq!(waits, 1, "it should wait once and then succeed");
        assert_eq!(calls, 2);
    }

    // Advertised but unreadable twice, readable on the third pass.
    #[test]
    fn an_advertised_format_that_fails_twice_is_recovered_on_the_third() {
        let (found, waits, calls) =
            drive(vec![(None, false), (None, false), (Some(PNG), false)], true);
        assert_eq!(found.as_deref(), Some(PNG));
        assert_eq!(waits, 2);
        assert_eq!(calls, 3);
    }

    // Bounded: a source that never becomes readable must not hold the capture
    // thread forever.
    #[test]
    fn retries_are_bounded() {
        let (found, waits, calls) = drive(
            vec![
                (None, false),
                (None, false),
                (None, false),
                (Some(PNG), false),
            ],
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
