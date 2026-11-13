//! Privacy-authorized callers project attachments into bounded, RAM-only previews.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::sync::OnceLock;
use crate::infrastructure::{attachments::path_from_uri, protected_resources as resources};

pub type VideoFrame = fn(&[u8], u32) -> Option<Vec<u8>>;
static VIDEO_FRAME: OnceLock<VideoFrame> = OnceLock::new();
pub fn install_video_decoder(decoder: VideoFrame) { let _ = VIDEO_FRAME.set(decoder); }

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}
fn video(name: &str) -> bool {
    std::path::Path::new(name).extension().and_then(|v| v.to_str())
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "mp4" | "m4v" | "mov" | "avi" | "mkv" | "webm" | "wmv"))
}
fn thumbnail(name: &str, bytes: &[u8], maximum: u32) -> Option<String> {
    let png = if video(name) { VIDEO_FRAME.get()?(bytes, maximum)? } else { bytes.to_vec() };
    let mime = image::guess_format(&png).ok()?.to_mime_type();
    resources::thumbnail_data_url(&format!("data:{mime};base64,{}", STANDARD.encode(png)), maximum).ok()
}

pub(crate) fn attachments(content: &str, maximum: u32) -> Result<(String, String), String> {
    use std::io::Read;
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
        for value in paths.into_iter().take(256) {
            let Ok(path) = path_from_uri(&value) else { continue; };
            let path = std::path::Path::new(&path);
            let Some(name) = path.file_name().and_then(|v| v.to_str()) else { continue; };
            // Only the first few attachments need thumbnails. Never read arbitrary
            // documents just to display their names, or follow remote/reparse paths.
            let visual = video(name) || image::ImageFormat::from_path(path).is_ok();
            let bytes = if entries.len() < 8 && visual {
                crate::filesystem::open_regular_nofollow(path).ok().and_then(|file| {
                    let length = file.metadata().ok()?.len();
                    if length > resources::MAX_RESOURCE_BYTES as u64 || length > remaining { return None; }
                    remaining -= length;
                    let mut bytes = Vec::new();
                    file.take(resources::MAX_RESOURCE_BYTES as u64 + 1).read_to_end(&mut bytes).ok()?;
                    (bytes.len() <= resources::MAX_RESOURCE_BYTES).then_some(bytes)
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
        if index < 8 && html.len() < 1024 * 1024 {
            let bytes = if video(&name) {
                if decoded_video { None } else { decoded_video = true; bytes }
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
    #[test]
    fn attachment_names_are_individual_and_escaped_even_when_missing() {
        let base = std::env::temp_dir();
        let paths = vec![base.join("first.png"), base.join("two & three.txt")];
        let content = paths.iter().map(|p| p.to_str().unwrap()).collect::<Vec<_>>().join("\n");
        let (names, html) = attachments(&content, 384).unwrap();
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
        let (names, html) = attachments(&content, 384).unwrap();
        assert_eq!(names, "first.png\nnotes.txt");
        assert_eq!(html.matches("<figure>").count(), 2);
        assert_eq!(html.matches("data:image/png;base64,").count(), 1);
        assert!(!html.contains("not exposed"));
        assert!(!html.contains(resources::FILES_PREFIX));
    }
}
