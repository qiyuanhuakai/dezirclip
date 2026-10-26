use super::content::detect_content_type;
use super::{CaptureContext, CaptureData, CaptureFailure, CaptureStage};
use crate::domain::models::ClipboardEntry;
use base64::{engine::general_purpose::STANDARD, Engine};

fn classify_file_path(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if [".gif", ".png", ".jpg", ".jpeg", ".bmp", ".webp"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        "image"
    } else if [".mp4", ".mkv", ".avi", ".mov", ".wmv", ".flv", ".webm"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        "video"
    } else {
        "file"
    }
}

pub(super) fn entry_from_capture(
    data: CaptureData,
    context: &CaptureContext<'_>,
) -> Result<ClipboardEntry, CaptureFailure> {
    let invalid_image = || CaptureFailure {
        stage: CaptureStage::Prepare,
        code: "InvalidClipboardImage".into(),
    };
    let (content_type, content, html_content) = match data {
        CaptureData::Text(text) => (detect_content_type(&text), text, None),
        CaptureData::RichText { text, html } => ("rich_text".into(), text, Some(html)),
        CaptureData::Image { data_url } => ("image".into(), data_url, None),
        CaptureData::EncodedImage { bytes, mime } => (
            "image".into(),
            format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
            None,
        ),
        CaptureData::RgbaImage {
            width,
            height,
            bytes,
        } => {
            let image =
                image::RgbaImage::from_raw(width, height, bytes).ok_or_else(invalid_image)?;
            let mut output = std::io::Cursor::new(Vec::new());
            image
                .write_to(&mut output, image::ImageFormat::Png)
                .map_err(|_| invalid_image())?;
            (
                "image".into(),
                format!(
                    "data:image/png;base64,{}",
                    STANDARD.encode(output.into_inner())
                ),
                None,
            )
        }
        CaptureData::Files(mut files) => {
            if files.len() == 1 {
                let path = files.pop().expect("length checked above");
                (classify_file_path(&path).into(), path, None)
            } else {
                ("file".into(), files.join("\n"), None)
            }
        }
    };
    let preview = if content_type == "image" {
        "[Image Content]".to_string()
    } else {
        let normalized = crate::database::normalize_text(&content);
        if normalized.chars().count() > 500 {
            let preview: String = normalized.chars().take(497).collect();
            format!("{}...", preview.replace('\n', " "))
        } else {
            normalized.replace('\n', " ")
        }
    };
    let is_external = matches!(content_type.as_str(), "file" | "video" | "image")
        && !content.starts_with("data:");
    Ok(ClipboardEntry {
        id: 0,
        content_type,
        content,
        html_content,
        source_app: context.source.app_name.clone(),
        source_app_path: context.source.process_path.clone(),
        timestamp: context.timestamp,
        preview,
        is_pinned: false,
        tags: Vec::new(),
        use_count: 0,
        is_external,
        pinned_order: 0,
        file_preview_exists: true,
        content_kinds: Vec::new(),
        ocr_text: None,
        ocr_status: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::capture::{CaptureSettings, CaptureSource};
    use std::collections::VecDeque;

    #[test]
    fn discovery_preserves_original_text_html_and_preview_contract() {
        let session = crate::api::SessionHistoryStore::new(VecDeque::new());
        let context = CaptureContext {
            source: CaptureSource::default(),
            timestamp: 1,
            settings: CaptureSettings::default(),
            session: &session,
            paused: false,
            sequential_echo: None,
        };
        for text in ["  first\r\n\tsecond\r\n", " \t\r\n", "trailing \n\n"] {
            let entry = entry_from_capture(CaptureData::Text(text.into()), &context).unwrap();
            assert_eq!(entry.content, text);
            assert!(!entry.preview.contains(['\r', '\n']));
        }
        let html = "\r\n<pre>  first\r\nsecond</pre>\r\n";
        let entry = entry_from_capture(
            CaptureData::RichText {
                text: "  first\r\nsecond\r\n".into(),
                html: html.into(),
            },
            &context,
        )
        .unwrap();
        assert_eq!(entry.html_content.as_deref(), Some(html));
        assert_eq!(entry.preview, "first second");
    }

    #[test]
    fn file_types_keep_case_insensitive_extension_rules() {
        for image in ["A.PNG", "a.gif", "a.jpg", "a.jpeg", "a.bmp", "a.webp"] {
            assert_eq!(classify_file_path(image), "image");
        }
        for video in [
            "a.Mp4", "a.mkv", "a.avi", "a.mov", "a.wmv", "a.flv", "a.webm",
        ] {
            assert_eq!(classify_file_path(video), "video");
        }
        for file in ["a.pdf", "a.txt", "a.zip", "a", "png.png.txt"] {
            assert_eq!(classify_file_path(file), "file");
        }
    }
}
