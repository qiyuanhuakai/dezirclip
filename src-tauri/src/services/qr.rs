use image::{ExtendedColorType, ImageEncoder, Luma};
use qrcode2::{EcLevel, QrCode};

const MIN_SIZE_PX: u32 = 64;
const MAX_SIZE_PX: u32 = 1024;

const EC_THRESHOLD_MEDIUM: usize = 100;
const EC_THRESHOLD_LONG: usize = 500;

#[derive(Debug)]
pub enum QrError {
    ContentTooLong,
    Encode(qrcode2::Error),
    Render(image::ImageError),
}

impl std::fmt::Display for QrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QrError::ContentTooLong => write!(f, "QR content exceeds maximum capacity"),
            QrError::Encode(e) => write!(f, "QR encode error: {e}"),
            QrError::Render(e) => write!(f, "QR render error: {e}"),
        }
    }
}

impl std::error::Error for QrError {}

impl From<qrcode2::Error> for QrError {
    fn from(e: qrcode2::Error) -> Self {
        match e {
            qrcode2::Error::DataTooLong => QrError::ContentTooLong,
            other => QrError::Encode(other),
        }
    }
}

impl From<image::ImageError> for QrError {
    fn from(e: image::ImageError) -> Self {
        QrError::Render(e)
    }
}

fn choose_ec_level(content_len: usize) -> EcLevel {
    if content_len < EC_THRESHOLD_MEDIUM {
        EcLevel::L
    } else if content_len < EC_THRESHOLD_LONG {
        EcLevel::M
    } else {
        EcLevel::Q
    }
}

fn encode_qr(content: &str) -> Result<QrCode, QrError> {
    let ec = choose_ec_level(content.len());
    Ok(QrCode::with_error_correction_level(
        content.as_bytes(),
        ec,
    )?)
}

pub fn generate_qr_png(content: &str, size_px: u32) -> Result<Vec<u8>, QrError> {
    let code = encode_qr(content)?;
    let size = size_px.clamp(MIN_SIZE_PX, MAX_SIZE_PX);
    let image = code
        .render::<Luma<u8>>()
        .min_dimensions(size, size)
        .build();
    let mut bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder.write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        ExtendedColorType::L8,
    )?;
    Ok(bytes)
}

pub fn generate_qr_svg(content: &str) -> Result<String, QrError> {
    let code = encode_qr(content)?;
    Ok(code.render::<qrcode2::render::svg::Color>().build())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn test_generate_qr_png_short() {
        let bytes = generate_qr_png("https://example.com/abc", 280).unwrap();
        assert!(bytes.len() > 100, "PNG should be > 100 bytes, got {}", bytes.len());
        assert_eq!(
            &bytes[..8],
            &PNG_MAGIC,
            "Output must start with PNG magic bytes"
        );
    }

    #[test]
    fn test_generate_qr_png_cjk() {
        let bytes = generate_qr_png("你好世界，这是一个二维码测试", 280).unwrap();
        assert!(bytes.len() > 100, "CJK PNG should be > 100 bytes");
        assert_eq!(&bytes[..8], &PNG_MAGIC, "CJK output must be valid PNG");
    }

    #[test]
    fn test_generate_qr_png_emoji() {
        let bytes = generate_qr_png("Hello 🎉🚀✨🌟", 280).unwrap();
        assert!(bytes.len() > 100, "Emoji PNG should be > 100 bytes");
        assert_eq!(&bytes[..8], &PNG_MAGIC, "Emoji output must be valid PNG");
    }

    #[test]
    fn test_generate_qr_svg() {
        let content = "https://example.com/qrcode-test";
        let svg = generate_qr_svg(content).unwrap();
        assert!(svg.contains("<svg"), "SVG output should contain <svg tag");
        assert!(
            svg.contains("</svg>"),
            "SVG output should contain closing </svg> tag"
        );
        // The content substring is rendered into the path data, not as text,
        // so verify the SVG header is well-formed.
        assert!(
            svg.contains("viewBox"),
            "SVG output should have a viewBox attribute"
        );
    }

    #[test]
    fn test_content_too_long() {
        let huge = "A".repeat(10_000);
        let result = generate_qr_png(&huge, 280);
        assert!(
            matches!(result, Err(QrError::ContentTooLong)),
            "Huge content should yield ContentTooLong, got {:?}",
            result.map(|_| "ok")
        );
    }
}
