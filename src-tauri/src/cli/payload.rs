use crate::domain::models::ClipboardEntry;
use base64::{engine::general_purpose::STANDARD, Engine};

pub fn mime_for(entry: &ClipboardEntry) -> &'static str {
    match entry.content_type.as_str() {
        "image" => "image/png",
        "file" | "files" | "video" => "text/uri-list",
        "html" | "rich_text" if entry.html_content.is_some() || entry.content_type == "html" => {
            "text/html"
        }
        _ => "text/plain",
    }
}

fn path_from_uri(value: &str) -> Result<String, String> {
    let Some(uri) = value.strip_prefix("file://") else {
        return Ok(value.to_owned());
    };
    let raw = if uri.starts_with('/') || (uri.as_bytes().get(1) == Some(&b':')) {
        uri.to_owned()
    } else {
        let (host, path) = uri.split_once('/').ok_or("file URI has no path")?;
        if host.eq_ignore_ascii_case("localhost") {
            format!("/{path}")
        } else {
            format!("//{host}/{path}")
        }
    };
    let path = urlencoding::decode(&raw)
        .map_err(|e| e.to_string())?
        .into_owned();
    #[cfg(target_os = "windows")]
    let path = if path.starts_with('/') && path.chars().nth(2) == Some(':') {
        path[1..].to_owned()
    } else {
        path
    };
    Ok(path)
}

pub fn image_bytes(content: &str) -> Result<Vec<u8>, String> {
    if content.starts_with("data:image/") {
        let (_, payload) = content.split_once(',').ok_or("invalid image data URL")?;
        STANDARD.decode(payload).map_err(|e| e.to_string())
    } else {
        let path = path_from_uri(content)?;
        std::fs::read(&path).map_err(|e| format!("read image: {e}"))
    }
}

pub fn files(content: &str) -> Result<Vec<String>, String> {
    let values = if content.trim_start().starts_with('[') {
        serde_json::from_str::<Vec<String>>(content)
            .map_err(|e| format!("invalid file list: {e}"))?
    } else {
        content.lines().map(str::to_string).collect()
    };
    let mut paths = Vec::new();
    for value in values {
        if value.is_empty() || value.starts_with('#') {
            continue;
        }
        let raw = path_from_uri(&value)?;
        paths.push(raw);
    }
    if paths.is_empty() {
        return Err("empty file list".to_string());
    }
    Ok(paths)
}

pub fn file_uri(path: &str) -> String {
    crate::infrastructure::native_clipboard::file_uri(path)
}

pub fn bytes_for(entry: &ClipboardEntry, mime: &str) -> Result<Vec<u8>, String> {
    match mime {
        "text/plain"
            if !matches!(
                entry.content_type.as_str(),
                "image" | "file" | "files" | "video"
            ) =>
        {
            Ok(entry.content.as_bytes().to_vec())
        }
        "text/html" => entry
            .html_content
            .as_ref()
            .or_else(|| (entry.content_type == "html").then_some(&entry.content))
            .map(|v| v.as_bytes().to_vec())
            .ok_or_else(|| "entry has no HTML representation".to_string()),
        "image/png" if entry.content_type == "image" => {
            let image = image::load_from_memory(&image_bytes(&entry.content)?)
                .map_err(|e| e.to_string())?;
            let mut out = std::io::Cursor::new(Vec::new());
            image
                .write_to(&mut out, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            Ok(out.into_inner())
        }
        "text/uri-list" if matches!(entry.content_type.as_str(), "file" | "files" | "video") => {
            Ok(files(&entry.content)?
                .iter()
                .map(|p| file_uri(p) + "\r\n")
                .collect::<String>()
                .into_bytes())
        }
        _ => Err(format!("entry has no {mime} representation")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_uri_authorities_and_literal_paths_are_distinct() {
        assert_eq!(
            path_from_uri("file://localhost/tmp/a%20b").unwrap(),
            "/tmp/a b"
        );
        assert_eq!(
            path_from_uri("file://server/share/a%20b").unwrap(),
            "//server/share/a b"
        );
        assert_eq!(path_from_uri("/tmp/100% done").unwrap(), "/tmp/100% done");
    }

    #[test]
    fn uri_list_preserves_spaces_and_unicode() {
        let uri = file_uri("/tmp/hello 世界.txt");
        assert_eq!(files(&uri).unwrap(), vec!["/tmp/hello 世界.txt"]);
        assert!(!uri.contains(' '));
        assert_eq!(
            files("/tmp/100% complete.txt").unwrap(),
            vec!["/tmp/100% complete.txt"]
        );
    }
}
