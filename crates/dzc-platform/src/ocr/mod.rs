pub mod linux;
pub mod windows;

#[derive(Debug)]
pub enum OcrError {
    NoOcrEngine,
    NoLanguages,
    ImageError(image::ImageError),
    OcrFailed(String),
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrError::NoOcrEngine => write!(f, "No OCR engine available. On Linux, install tesseract-ocr and language packs such as tesseract-ocr-eng."),
            OcrError::NoLanguages => write!(f, "No OCR language pack installed"),
            OcrError::ImageError(e) => write!(f, "Image decode error: {e}"),
            OcrError::OcrFailed(msg) => write!(f, "OCR failed: {msg}"),
        }
    }
}

impl std::error::Error for OcrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OcrError::ImageError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<image::ImageError> for OcrError {
    fn from(e: image::ImageError) -> Self {
        OcrError::ImageError(e)
    }
}

#[cfg(target_os = "linux")]
pub use linux::OcrService;

#[cfg(target_os = "windows")]
pub use windows::OcrService;
