use base64::{engine::general_purpose::STANDARD, Engine};
use dzc_core::services::capture::{
    CaptureData, CaptureFailure, CaptureReadMetadata, CaptureReadRequest, CaptureReader,
    CaptureSkip, CaptureStage,
};
use std::hash::{Hash, Hasher};

#[derive(Default)]
pub struct NativeCaptureReader {
    metadata: CaptureReadMetadata,
    empty: Option<CaptureSkip>,
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
    fn image(&mut self, image: Pixels) -> CaptureData {
        let hash = fingerprint(image.bytes.as_slice());
        self.metadata = CaptureReadMetadata {
            fingerprint: Some(hash),
            echo_hash: Some(hash),
            image: true,
            files: false,
        };
        CaptureData::RgbaImage {
            width: image.width,
            height: image.height,
            bytes: image.bytes,
        }
    }
}

impl CaptureReader for NativeCaptureReader {
    fn read(&mut self, request: CaptureReadRequest) -> Result<Option<CaptureData>, CaptureFailure> {
        self.metadata = CaptureReadMetadata::default();
        self.empty = None;
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
                        let image = image.into_rgba8();
                        let (width, height) = image.dimensions();
                        return Ok(Some(self.image(Pixels {
                            width,
                            height,
                            bytes: image.into_raw(),
                        })));
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
            if text.as_ref().is_some_and(|text| !text.trim().is_empty()) {
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
