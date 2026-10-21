//! Fingerprints shared by capture and explicit copy. These values are local echo
//! hints, not content identities or authorization evidence.
use dzc_core::ports::{ClipboardPayload, PlatformResult};
use dzc_protocol::{Capability, PlatformError, UnsupportedReason};
use std::hash::{Hash, Hasher};

const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardEcho {
    pub primary: u64,
    pub alternative: u64,
}

pub(super) fn fingerprint(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn rejected() -> PlatformError {
    PlatformError::failed(Capability::Write)
}

fn decoded_pixels(bytes: &[u8]) -> PlatformResult<image::RgbaImage> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(rejected());
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| rejected())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);
    // Check the RGBA allocation too: a smaller decoder output may expand during
    // conversion (for example, an 8-bit grayscale image).
    let dimensions = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| rejected())?
        .into_dimensions()
        .map_err(|_| rejected())?;
    if u64::from(dimensions.0)
        .checked_mul(u64::from(dimensions.1))
        .and_then(|pixels| pixels.checked_mul(4))
        .is_none_or(|bytes| bytes > MAX_DECODED_BYTES)
    {
        return Err(rejected());
    }
    reader
        .decode()
        .map(image::DynamicImage::into_rgba8)
        .map_err(|_| rejected())
}

fn text_echo(text: &str) -> ClipboardEcho {
    #[cfg(target_os = "windows")]
    let text = text.split('\0').next().unwrap_or_default();
    ClipboardEcho {
        primary: fingerprint(text),
        alternative: 0,
    }
}

/// Compute before an explicit history copy; clear the caller's saved hint if
/// writing fails. No native connection, clipboard read, worker or database is
/// opened here. Generated writes must not record this suppression hint.
///
/// Protected variants return None without inspecting their body or exports:
/// their native source-exclusion hints and authorization leases are separate.
/// File-image decoding is bounded and reads only the single supplied regular
/// file; an unreadable or oversized image is an error, never a guessed hash.
pub fn clipboard_echo(payload: &ClipboardPayload) -> PlatformResult<Option<ClipboardEcho>> {
    let echo = match payload {
        ClipboardPayload::Private { .. } | ClipboardPayload::ProtectedFiles { .. } => {
            return Ok(None)
        }
        ClipboardPayload::Text(text) | ClipboardPayload::Html { text, .. } => text_echo(text),
        ClipboardPayload::Files(paths) => {
            let length = paths
                .iter()
                .try_fold(0usize, |size, path| {
                    size.checked_add(path.len())?.checked_add(1)
                })
                .ok_or_else(rejected)?;
            if length > MAX_ENCODED_BYTES {
                return Err(rejected());
            }
            let primary = fingerprint(paths.join("\n"));
            #[cfg(target_os = "linux")]
            let alternative = if let [path] = paths.as_slice() {
                let lower = path.to_ascii_lowercase();
                if [".png", ".jpg", ".jpeg", ".bmp", ".webp", ".gif"]
                    .iter()
                    .any(|suffix| lower.ends_with(suffix))
                {
                    let bytes = crate::local_file::read_bounded_regular(
                        std::path::Path::new(path),
                        MAX_ENCODED_BYTES,
                    )
                    .map_err(|_| rejected())?;
                    fingerprint(decoded_pixels(&bytes)?.as_raw().as_slice())
                } else {
                    0
                }
            } else {
                0
            };
            #[cfg(target_os = "windows")]
            let alternative = 0;
            ClipboardEcho {
                primary,
                alternative,
            }
        }
        ClipboardPayload::Image { bytes, mime } => {
            if !matches!(
                mime.as_str(),
                "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp"
            ) {
                return Err(PlatformError::unsupported(
                    Capability::Write,
                    UnsupportedReason::UnsupportedRepresentation,
                ));
            }
            let pixels = decoded_pixels(bytes)?;
            let primary = fingerprint(pixels.as_raw().as_slice());
            #[cfg(target_os = "windows")]
            let alternative = {
                let mut pixels = pixels.into_raw();
                super::windows_clipboard::normalize_zero_alpha(&mut pixels);
                let normalized = fingerprint(pixels.as_slice());
                if normalized == primary {
                    0
                } else {
                    normalized
                }
            };
            #[cfg(target_os = "linux")]
            let alternative = fingerprint(bytes);
            ClipboardEcho {
                primary,
                alternative,
            }
        }
    };
    Ok(Some(echo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(pixels: &[u8]) -> Vec<u8> {
        let image = image::RgbaImage::from_raw(2, 1, pixels.to_vec()).unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn text_and_html_preserve_raw_whitespace_and_share_capture_hash() {
        for text in ["  copied\r\n\tline\r\n", " \t\r\n", "汉字😀"] {
            let plain = clipboard_echo(&ClipboardPayload::Text(text.into()))
                .unwrap()
                .unwrap();
            let html = clipboard_echo(&ClipboardPayload::Html {
                text: text.into(),
                html: "<p>different markup</p>".into(),
            })
            .unwrap()
            .unwrap();
            assert_eq!(plain, html);
            assert_eq!(plain.primary, fingerprint(text));
            assert_eq!(plain.alternative, 0);
        }
        assert_ne!(text_echo("x\n"), text_echo("x\r\n"));
    }

    #[test]
    fn file_order_and_image_rgba_use_capture_fingerprints() {
        let files = vec![
            "/synthetic/first.txt".into(),
            "/synthetic/second.txt".into(),
        ];
        assert_eq!(
            clipboard_echo(&ClipboardPayload::Files(files.clone()))
                .unwrap()
                .unwrap()
                .primary,
            fingerprint(files.join("\n"))
        );
        let pixels = [1, 2, 3, 255, 9, 8, 7, 128];
        let bytes = png(&pixels);
        let echo = clipboard_echo(&ClipboardPayload::Image {
            bytes: bytes.clone(),
            mime: "image/png".into(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(echo.primary, fingerprint(pixels.as_slice()));
        #[cfg(target_os = "linux")]
        assert_eq!(echo.alternative, fingerprint(bytes));
        #[cfg(target_os = "windows")]
        assert_eq!(echo.alternative, 0);
    }

    #[test]
    fn invalid_image_mime_and_decoding_fail_before_native_access() {
        for (bytes, mime) in [
            (vec![1, 2, 3], "image/png"),
            (png(&[0; 8]), "application/unknown"),
        ] {
            assert!(clipboard_echo(&ClipboardPayload::Image {
                bytes,
                mime: mime.into()
            })
            .is_err());
        }
    }

    #[test]
    fn image_header_cannot_request_an_unbounded_rgba_allocation() {
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Bmp).unwrap();
        let mut bytes = bytes.into_inner();
        bytes[18..22].copy_from_slice(&10_000i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&10_000i32.to_le_bytes());
        let dimensions = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap();
        assert_eq!(dimensions, (10_000, 10_000));
        assert!(decoded_pixels(&bytes).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_x11_written_payloads_match_reader_echo_and_only_explicit_copy_is_skipped() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        use crate::{capture::NativeCaptureReader, runtime::EmbeddedClipboardWriter};
        use dzc_core::{
            capture::{
                CaptureNotificationState, CaptureReadRequest, CaptureReader, CaptureSkip,
                OwnClipboardCopy,
            },
            ports::ClipboardWriter,
        };
        let directory =
            std::env::temp_dir().join(format!("dezirclip-echo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("ordinary.txt");
        std::fs::write(&file, b"synthetic file").unwrap();
        let pixels = [1, 2, 3, 255, 7, 8, 9, 128];
        let image_path = directory.join("ordinary.png");
        std::fs::write(&image_path, png(&pixels)).unwrap();
        let payloads = [
            ClipboardPayload::Text("  synthetic text\r\n\t".into()),
            ClipboardPayload::Html {
                text: " \t\r\n".into(),
                html: "<p><br></p>".into(),
            },
            ClipboardPayload::Image {
                bytes: png(&pixels),
                mime: "image/png".into(),
            },
            ClipboardPayload::Files(vec![file.to_string_lossy().into_owned()]),
            ClipboardPayload::Files(vec![image_path.to_string_lossy().into_owned()]),
        ];
        let writer = EmbeddedClipboardWriter::default();
        for payload in payloads {
            let echo = clipboard_echo(&payload).unwrap().unwrap();
            writer.write_suppressing_recapture(payload).unwrap();
            let mut reader = NativeCaptureReader::default();
            assert!(reader
                .read(CaptureReadRequest {
                    files: true,
                    rich_text: true
                })
                .unwrap()
                .is_some());
            let metadata = reader.metadata();
            assert!(
                metadata
                    .echo_hash
                    .is_some_and(|value| value == echo.primary || value == echo.alternative),
                "{metadata:?} != {echo:?}"
            );
            let own = OwnClipboardCopy {
                primary: echo.primary,
                alternative: echo.alternative,
                age_seconds: 0,
            };
            assert_eq!(
                CaptureNotificationState::default().inspect(metadata, 1000, own, false),
                Err(CaptureSkip::OwnClipboard)
            );
            assert_eq!(
                CaptureNotificationState::default().inspect(
                    metadata,
                    1000,
                    OwnClipboardCopy::default(),
                    false
                ),
                Ok(())
            );
        }
        writer
            .write(ClipboardPayload::Text(
                "generated OCR must remain capturable".into(),
            ))
            .unwrap();
        let mut reader = NativeCaptureReader::default();
        assert!(reader
            .read(CaptureReadRequest {
                files: true,
                rich_text: true
            })
            .unwrap()
            .is_some());
        assert_eq!(
            CaptureNotificationState::default().inspect(
                reader.metadata(),
                1000,
                OwnClipboardCopy::default(),
                false
            ),
            Ok(())
        );
        writer.stop().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_zero_alpha_and_nul_match_native_reader_conversion() {
        let pixels = [1, 2, 3, 0, 4, 5, 6, 0];
        let echo = clipboard_echo(&ClipboardPayload::Image {
            bytes: png(&pixels),
            mime: "image/png".into(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(echo.primary, fingerprint(pixels.as_slice()));
        assert_eq!(
            echo.alternative,
            fingerprint([1u8, 2, 3, 255, 4, 5, 6, 255].as_slice())
        );
        assert_eq!(text_echo("first\0second").primary, fingerprint("first"));
    }
}
