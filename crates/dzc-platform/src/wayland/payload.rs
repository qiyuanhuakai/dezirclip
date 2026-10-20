use super::{MimeData, WaylandError};
use std::sync::{Arc, OnceLock};

pub(crate) fn own_marker() -> &'static str {
    static MIME: OnceLock<String> = OnceLock::new();
    MIME.get_or_init(|| {
        format!(
            "application/x-dezirclip-owned-{}",
            uuid::Uuid::new_v4().simple()
        )
    })
}

pub fn representations(
    payload: crate::native_clipboard::Payload,
) -> Result<Vec<MimeData>, WaylandError> {
    representations_with_intent(payload, false)
}
pub fn representations_with_intent(
    payload: crate::native_clipboard::Payload,
    suppress_recapture: bool,
) -> Result<Vec<MimeData>, WaylandError> {
    use crate::native_clipboard::Payload;
    let mut result = Vec::new();
    let text = |value: String, result: &mut Vec<MimeData>| {
        let bytes: Arc<[u8]> = value.into_bytes().into();
        for mime in ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"] {
            result.push(MimeData {
                mime: mime.into(),
                bytes: bytes.clone(),
            });
        }
    };
    match payload {
        Payload::Text(value) => text(value, &mut result),
        Payload::Html {
            html,
            text: fallback,
        } => {
            text(fallback, &mut result);
            result.push(MimeData {
                mime: "text/html".into(),
                bytes: html.into_bytes().into(),
            });
        }
        Payload::Files(paths) => {
            if paths.is_empty() {
                return Err(WaylandError::UnsupportedMime);
            }
            let uris = paths
                .iter()
                .map(|path| crate::native_clipboard::file_uri(path) + "\r\n")
                .collect::<String>();
            result.push(MimeData {
                mime: "text/uri-list".into(),
                bytes: uris.as_bytes().into(),
            });
            result.push(MimeData {
                mime: "x-special/gnome-copied-files".into(),
                bytes: format!("copy\n{uris}").into_bytes().into(),
            });
        }
        Payload::Image(bytes) => {
            let mime =
                match image::guess_format(&bytes).map_err(|_| WaylandError::UnsupportedMime)? {
                    image::ImageFormat::Png => "image/png",
                    image::ImageFormat::Jpeg => "image/jpeg",
                    image::ImageFormat::Gif => "image/gif",
                    image::ImageFormat::WebP => "image/webp",
                    image::ImageFormat::Bmp => "image/bmp",
                    _ => return Err(WaylandError::UnsupportedMime),
                };
            result.push(MimeData {
                mime: mime.into(),
                bytes: bytes.into(),
            });
        }
    }
    if suppress_recapture {
        result.push(MimeData {
            mime: own_marker().into(),
            bytes: Arc::from([]),
        });
    }
    super::validate_payloads(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_aliases_share_bytes_and_html_keeps_its_distinct_representation() {
        let values = representations(crate::native_clipboard::Payload::Html {
            text: "plain".into(),
            html: "<b>plain</b>".into(),
        })
        .unwrap();
        assert!(Arc::ptr_eq(&values[0].bytes, &values[1].bytes));
        assert_eq!(
            &*values.iter().find(|x| x.mime == "text/html").unwrap().bytes,
            b"<b>plain</b>"
        );
        assert!(values.iter().all(|value| value.mime != own_marker()));
        let copied = representations_with_intent(
            crate::native_clipboard::Payload::Text("existing history copy".into()),
            true,
        )
        .unwrap();
        assert_eq!(copied.last().unwrap().mime, own_marker());
    }
}
