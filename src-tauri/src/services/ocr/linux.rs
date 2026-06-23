use crate::services::ocr::OcrError;

/// Linux OCR placeholder.
///
/// Real OCR on Linux (Tesseract WASM, native tesseract crate, or any
/// alternative) is deferred to a later phase. To keep the cross-platform
/// `services::ocr::OcrService` surface uniform, both `new()` and `recognize()`
/// always report `OcrError::NoOcrEngine`. Callers should treat this as
/// "OCR is not yet supported on Linux" and either disable the feature or
/// surface a localized message to the user.
#[derive(Debug)]
pub struct OcrService;

impl OcrService {
    pub fn new() -> Result<Self, OcrError> {
        Err(OcrError::NoOcrEngine)
    }

    pub fn recognize(&self, _png_bytes: &[u8]) -> Result<String, OcrError> {
        Err(OcrError::NoOcrEngine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linux_ocr_service_new_returns_no_engine() {
        let result = OcrService::new();
        assert!(
            matches!(result, Err(OcrError::NoOcrEngine)),
            "OcrService::new() must return Err(NoOcrEngine) on Linux, got {:?}",
            result.map(|s| format!("constructed: {s:?}"))
        );
    }

    #[test]
    fn test_linux_ocr_service_recognize_returns_no_engine() {
        let svc = OcrService;
        let png = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        png.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let result = svc.recognize(&buf.into_inner());
        assert!(
            matches!(result, Err(OcrError::NoOcrEngine)),
            "recognize() on valid PNG must return Err(NoOcrEngine) on Linux, got {:?}",
            result.map(|t| format!("text={t:?}"))
        );
    }

    #[test]
    fn test_linux_ocr_service_recognize_empty_bytes() {
        let svc = OcrService;
        let result = svc.recognize(&[]);
        assert!(
            matches!(result, Err(OcrError::NoOcrEngine)),
            "recognize() on empty bytes must return Err(NoOcrEngine) on Linux, got {:?}",
            result.map(|t| format!("text={t:?}"))
        );
    }

    #[test]
    fn test_linux_ocr_service_recognize_garbage_bytes() {
        let svc = OcrService;
        let garbage: &[u8] = &[0xFF, 0x00, 0xAB, 0xCD, 0xEE, 0x12, 0x34, 0x56];
        let result = svc.recognize(garbage);
        assert!(
            matches!(result, Err(OcrError::NoOcrEngine)),
            "recognize() on garbage bytes must return Err(NoOcrEngine) on Linux, got {:?}",
            result.map(|t| format!("text={t:?}"))
        );
    }

    #[test]
    fn test_linux_ocr_service_error_message_mentions_linux() {
        let msg = OcrError::NoOcrEngine.to_string();
        let lowered = msg.to_lowercase();
        assert!(
            lowered.contains("linux") || lowered.contains("not supported"),
            "Error message should mention 'Linux' or 'not supported', got {:?}",
            msg
        );
    }
}
