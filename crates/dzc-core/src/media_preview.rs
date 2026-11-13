//! Privacy-authorized callers project attachments into bounded, RAM-only previews.
use std::sync::OnceLock;
use crate::infrastructure::{attachments::path_from_uri, protected_resources as resources};

pub type VideoFrame = fn(&[u8], u32) -> Option<Vec<u8>>;
static VIDEO_FRAME: OnceLock<VideoFrame> = OnceLock::new();
pub fn install_video_decoder(decoder: VideoFrame) { let _ = VIDEO_FRAME.set(decoder); }
pub type AttachmentReader = fn(&std::path::Path, usize) -> Option<Vec<u8>>;
static READ_ATTACHMENT: OnceLock<AttachmentReader> = OnceLock::new();
pub fn install_attachment_reader(reader: AttachmentReader) { let _ = READ_ATTACHMENT.set(reader); }

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}
fn video(name: &str) -> bool {
    std::path::Path::new(name).extension().and_then(|v| v.to_str())
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "mp4" | "m4v" | "mov" | "avi" | "mkv" | "webm" | "wmv"))
}
fn thumbnail(name: &str, bytes: &[u8], maximum: u32) -> Option<String> {
    if video(name) {
        let png = VIDEO_FRAME.get()?(bytes, maximum)?;
        resources::thumbnail_bytes(&png, maximum).ok()
    } else {
        resources::thumbnail_bytes(bytes, maximum).ok()
    }
}

pub(crate) fn attachments(content: &str, maximum: u32, include_video_frame: bool) -> Result<(String, String), String> {
    let mut entries = Vec::new();
    if content.starts_with(resources::FILES_PREFIX) {
        for file in resources::decode_files(content)? {
            entries.push((file.name, Some(file.bytes)));
        }
    } else {
        let mut remaining = 32 * 1024 * 1024;
        let paths: Vec<String> = if content.trim_start().starts_with('[') {
            serde_json::from_str(content).map_err(|_| "AttachmentReadRejected")?
        } else { content.lines().filter(|v| !v.trim().is_empty()).map(str::to_owned).collect() };
        let mut video_read = false;
        for value in paths.into_iter().take(256) {
            let Ok(path) = path_from_uri(&value) else { continue; };
            let path = std::path::Path::new(&path);
            let Some(name) = path.file_name().and_then(|v| v.to_str()) else { continue; };
            // Only the first few attachments need thumbnails. Never read arbitrary
            // documents just to display their names, or follow remote/reparse paths.
            let is_video = video(name);
            let visual = if is_video { include_video_frame && !video_read } else { image::ImageFormat::from_path(path).is_ok() };
            let bytes = if entries.len() < 4 && visual {
                video_read |= is_video;
                READ_ATTACHMENT.get().and_then(|read| read(path, remaining.min(resources::MAX_RESOURCE_BYTES))).map(|bytes| {
                    remaining = remaining.saturating_sub(bytes.len());
                    bytes
                })
            } else { None };
            entries.push((name.to_owned(), bytes));
        }
    }
    let names = entries.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join("\n");
    let bound = if entries.len() > 1 { maximum.min(192) } else { maximum.min(560) };
    let mut html = String::new();
    let mut decoded_video = false;
    for (index, (name, bytes)) in entries.into_iter().enumerate() {
        html.push_str("<figure>");
        if index < 4 && html.len() < 1024 * 1024 {
            let bytes = if video(&name) {
                if !include_video_frame || decoded_video { None } else { decoded_video = true; bytes }
            } else { bytes };
            if let Some(src) = bytes.and_then(|bytes| thumbnail(&name, &bytes, bound)) {
                if src.len() < 512 * 1024 {
                    html.push_str(&format!("<img src=\"{src}\" alt=\"{}\">", escape(&name)));
                }
            }
        }
        html.push_str(&format!("<figcaption>{}</figcaption></figure>", escape(&name)));
    }
    Ok((names, html))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    #[test]
    fn video_cards_do_not_read_or_decode_video_but_hover_requests_one_frame() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static READS: AtomicUsize = AtomicUsize::new(0);
        static DECODES: AtomicUsize = AtomicUsize::new(0);
        install_attachment_reader(|path, _| {
            if path.extension().is_some_and(|ext| ext == "mp4") {
                READS.fetch_add(1, Ordering::SeqCst);
                Some(b"synthetic video".to_vec())
            } else { None }
        });
        install_video_decoder(|_, maximum| {
            DECODES.fetch_add(1, Ordering::SeqCst);
            let mut png = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(maximum, 80, image::Rgba([20, 40, 60, 255]))
                .write_to(&mut png, image::ImageFormat::Png).unwrap();
            Some(png.into_inner())
        });
        let content = ["first.mp4", "second.mp4"].map(|name| std::env::temp_dir().join(name).to_string_lossy().into_owned()).join("\n");
        let (names, card) = attachments(&content, 480, false).unwrap();
        assert_eq!(names, "first.mp4\nsecond.mp4");
        assert!(!card.contains("<img"));
        assert_eq!(READS.load(Ordering::SeqCst), 0);
        assert_eq!(DECODES.load(Ordering::SeqCst), 0);
        let (_, hover) = attachments(&content, 560, true).unwrap();
        assert_eq!(hover.matches("<img").count(), 1);
        assert_eq!(READS.load(Ordering::SeqCst), 1);
        assert_eq!(DECODES.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn large_image_groups_export_only_four_bounded_thumbnails() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(1200, 800, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut png, image::ImageFormat::Png).unwrap();
        let encoded = STANDARD.encode(png.into_inner());
        let files: Vec<_> = (0..6).map(|i| serde_json::json!({"name":format!("{i}.png"),"data":encoded})).collect();
        let content = format!("{}{}", resources::FILES_PREFIX, serde_json::json!({"version":1,"files":files}));
        let (names, html) = attachments(&content, 480, false).unwrap();
        assert_eq!(names.lines().count(), 6);
        assert_eq!(html.matches("<img").count(), 4);
        for encoded in html.split("data:image/png;base64,").skip(1) {
            let bytes = STANDARD.decode(encoded.split('"').next().unwrap()).unwrap();
            let image = image::load_from_memory(&bytes).unwrap();
            assert_eq!((image.width(), image.height()), (192, 128));
        }
    }
    #[test]
    fn attachment_names_are_individual_and_escaped_even_when_missing() {
        let base = std::env::temp_dir();
        let paths = vec![base.join("first.png"), base.join("two & three.txt")];
        let content = paths.iter().map(|p| p.to_str().unwrap()).collect::<Vec<_>>().join("\n");
        let (names, html) = attachments(&content, 384, false).unwrap();
        assert_eq!(names, "first.png\ntwo & three.txt");
        assert!(html.contains("two &amp; three.txt"));
        assert_eq!(html.matches("<figure>").count(), 2);
        assert!(!html.contains(base.to_str().unwrap()));
        assert!(!html.contains("src="));
    }
    #[test]
    fn archived_images_share_one_gallery_without_exporting_plaintext_files() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(8, 6, image::Rgba([10, 20, 30, 255])))
            .write_to(&mut png, image::ImageFormat::Png).unwrap();
        let content = format!("{}{}", resources::FILES_PREFIX, serde_json::json!({"version":1,"files":[
            {"name":"first.png","data": STANDARD.encode(png.into_inner())},
            {"name":"notes.txt","data": STANDARD.encode(b"not exposed in a preview")}
        ]}));
        let (names, html) = attachments(&content, 384, false).unwrap();
        assert_eq!(names, "first.png\nnotes.txt");
        assert_eq!(html.matches("<figure>").count(), 2);
        assert_eq!(html.matches("data:image/png;base64,").count(), 1);
        assert!(!html.contains("not exposed"));
        assert!(!html.contains(resources::FILES_PREFIX));
    }
}
