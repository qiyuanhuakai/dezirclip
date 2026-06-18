use base64::Engine;
use crate::services::qr;

/// Generate a QR code PNG for `content` at the given pixel size and return
/// the image as a `data:image/png;base64,...` data-URL string.
///
/// The frontend calls this via `invoke("generate_qr_png", { content, sizePx })`.
#[tauri::command]
pub fn generate_qr_png(content: String, size_px: u32) -> Result<String, String> {
    let bytes = qr::generate_qr_png(&content, size_px).map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/png;base64,{b64}"))
}

/// Generate a QR code SVG string for `content`.
///
/// The frontend calls this via `invoke("generate_qr_svg", { content })`.
#[tauri::command]
pub fn generate_qr_svg(content: String) -> Result<String, String> {
    qr::generate_qr_svg(&content).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_qr_png_returns_data_url() {
        let result = generate_qr_png("https://example.com".to_string(), 256);
        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(
            url.starts_with("data:image/png;base64,"),
            "expected data URL prefix, got: {url}"
        );
        // base64 part should be non-trivial (> 100 chars for a QR PNG)
        assert!(url.len() > 50, "data URL too short: {}", url.len());
    }

    #[test]
    fn test_generate_qr_svg_returns_svg() {
        let result = generate_qr_svg("https://example.com".to_string());
        assert!(result.is_ok());
        let svg = result.unwrap();
        assert!(svg.contains("<svg"), "SVG should contain <svg tag");
        assert!(svg.contains("</svg>"), "SVG should contain closing tag");
    }

    #[test]
    fn test_generate_qr_png_empty_content() {
        let result = generate_qr_png(String::new(), 256);
        assert!(result.is_ok(), "empty content should still produce a QR code");
    }
}
