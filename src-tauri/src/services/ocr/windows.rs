use image::DynamicImage;

use crate::services::ocr::OcrError;

#[cfg(target_os = "windows")]
use windows::Windows::Globalization::Language;
#[cfg(target_os = "windows")]
use windows::Windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
#[cfg(target_os = "windows")]
use windows::Windows::Media::Ocr::OcrEngine;
#[cfg(target_os = "windows")]
use windows::Windows::Storage::Streams::DataWriter;
#[cfg(target_os = "windows")]
use windows_core::HSTRING;

/// Cross-platform OCR façade. On Windows it wraps `Windows.Media.Ocr.OcrEngine`;
/// on Linux / other platforms it is a stub that always reports `NoOcrEngine`.
pub struct OcrService {
    #[cfg(target_os = "windows")]
    engine: OcrEngine,
}

impl OcrService {
    #[cfg(target_os = "windows")]
    pub fn new() -> Result<Self, OcrError> {
        if let Ok(engine) = OcrEngine::TryCreateFromUserProfileLanguages() {
            return Ok(Self { engine });
        }
        let lang = Language::CreateLanguage(&HSTRING::from("en-US"))
            .map_err(|_| OcrError::NoLanguages)?;
        let engine = OcrEngine::TryCreateFromLanguage(&lang)
            .map_err(|_| OcrError::NoOcrEngine)?;
        Ok(Self { engine })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn new() -> Result<Self, OcrError> {
        Ok(Self {})
    }

    /// Decode `png_bytes` and run OCR over the image.
    ///
    /// Image decoding is shared across platforms so malformed input surfaces as
    /// `OcrError::ImageError`. The actual OCR step is platform-gated; on non-Windows
    /// a successful decode still resolves to `OcrError::NoOcrEngine`.
    pub fn recognize(&self, png_bytes: &[u8]) -> Result<String, OcrError> {
        let img = image::load_from_memory(png_bytes)?;
        self.run_ocr(&img)
    }

    #[cfg(target_os = "windows")]
    fn run_ocr(&self, img: &DynamicImage) -> Result<String, OcrError> {
        let bitmap = dynamic_image_to_software_bitmap(img)?;
        let async_op = self
            .engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| OcrError::OcrFailed(format!("RecognizeAsync failed: {e}")))?;
        let result = async_op
            .get()
            .map_err(|e| OcrError::OcrFailed(format!("OCR operation failed: {e}")))?;
        let text = result
            .Text()
            .map_err(|e| OcrError::OcrFailed(format!("Text() failed: {e}")))?;
        Ok(text.to_string())
    }

    #[cfg(not(target_os = "windows"))]
    fn run_ocr(&self, _img: &DynamicImage) -> Result<String, OcrError> {
        Err(OcrError::NoOcrEngine)
    }
}

#[cfg(target_os = "windows")]
fn dynamic_image_to_software_bitmap(img: &DynamicImage) -> Result<SoftwareBitmap, OcrError> {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let bytes = rgba.into_raw();
    let writer = DataWriter::new()
        .map_err(|e| OcrError::OcrFailed(format!("DataWriter::new failed: {e}")))?;
    writer
        .WriteBytes(&bytes)
        .map_err(|e| OcrError::OcrFailed(format!("WriteBytes failed: {e}")))?;
    let buffer = writer
        .DetachBuffer()
        .map_err(|e| OcrError::OcrFailed(format!("DetachBuffer failed: {e}")))?;
    SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Rgba8,
        width as i32,
        height as i32,
    )
    .map_err(|e| OcrError::OcrFailed(format!("SoftwareBitmap::CreateCopyFromBuffer failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_service_new() {
        let result = OcrService::new();
        assert!(
            result.is_ok(),
            "OcrService::new() should succeed (empty stub on non-Windows, real engine on Windows where installed), got {:?}",
            result.err()
        );
    }

    #[test]
    fn test_ocr_service_new_linux_stub_returns_no_engine_on_recognize() {
        #[cfg(not(target_os = "windows"))]
        {
            let svc = OcrService::new().expect("Linux stub should still construct");
            let png = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
            let mut buf = std::io::Cursor::new(Vec::<u8>::new());
            png.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            let result = svc.recognize(&buf.into_inner());
            assert!(
                matches!(result, Err(OcrError::NoOcrEngine)),
                "Linux stub must reject OCR with NoOcrEngine, got {:?}",
                result.map(|t| format!("text={t:?}"))
            );
        }
    }

    #[test]
    fn test_ocr_error_display() {
        let cases = [
            (OcrError::NoOcrEngine, "No OCR engine"),
            (OcrError::NoLanguages, "No OCR language"),
            (
                OcrError::OcrFailed("kaboom".to_string()),
                "OCR failed: kaboom",
            ),
        ];
        for (err, expected_substr) in cases {
            let msg = err.to_string();
            assert!(
                msg.contains(expected_substr),
                "Display for {:?} should contain {:?}, got {:?}",
                err,
                expected_substr,
                msg
            );
        }
    }

    #[test]
    fn test_recognize_empty_bytes() {
        let service = OcrService::new().expect("service should construct on all platforms");
        let result = service.recognize(&[]);
        assert!(
            matches!(result, Err(OcrError::ImageError(_))),
            "Empty bytes must yield ImageError, got {:?}",
            result.map(|t| format!("text={t:?}"))
        );
    }

    #[test]
    fn test_recognize_invalid_png() {
        let service = OcrService::new().expect("service should construct on all platforms");
        let garbage: &[u8] = &[0xFF, 0x00, 0xAB, 0xCD, 0xEE, 0x12, 0x34, 0x56];
        let result = service.recognize(garbage);
        assert!(
            matches!(result, Err(OcrError::ImageError(_))),
            "Garbage bytes must yield ImageError, got {:?}",
            result.map(|t| format!("text={t:?}"))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_dynamic_image_to_software_bitmap_zero_sized() {
        use image::{ImageBuffer, Rgba};
        let img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(1, 1, Rgba([0, 0, 0, 0])));
        let bitmap = dynamic_image_to_software_bitmap(&img).expect("bitmap build");
        assert_eq!(bitmap.PixelWidth().unwrap(), 1);
        assert_eq!(bitmap.PixelHeight().unwrap(), 1);
    }
}
