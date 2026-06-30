use std::path::PathBuf;
use std::process::Command;

use crate::services::ocr::OcrError;

const DEFAULT_TESSERACT_LANG: &str = "eng+chi_sim";
const OCR_LANG_ENV: &str = "DEZIRCLIP_OCR_LANG";

#[derive(Debug)]
pub struct OcrService {
    command: String,
    language: String,
}

impl OcrService {
    pub fn new() -> Result<Self, OcrError> {
        let command = "tesseract".to_string();
        let available = Command::new(&command)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if !available {
            return Err(OcrError::NoOcrEngine);
        }

        Ok(Self {
            command,
            language: ocr_language(),
        })
    }

    pub fn recognize(&self, png_bytes: &[u8]) -> Result<String, OcrError> {
        image::load_from_memory(png_bytes)?;
        let path = write_temp_png(png_bytes)?;
        let result = self.run_tesseract(&path);
        let _ = std::fs::remove_file(&path);
        result
    }

    fn run_tesseract(&self, path: &std::path::Path) -> Result<String, OcrError> {
        match run_tesseract_command(&self.command, path, &self.language) {
            Ok(text) => Ok(text),
            Err(err) if self.language != "eng" => run_tesseract_command(&self.command, path, "eng")
                .map_err(|_| err),
            Err(err) => Err(err),
        }
    }
}

fn ocr_language() -> String {
    std::env::var(OCR_LANG_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_TESSERACT_LANG.to_string())
}

fn temp_png_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("dezirclip-ocr-{}-{nanos}.png", std::process::id()))
}

fn write_temp_png(png_bytes: &[u8]) -> Result<PathBuf, OcrError> {
    let path = temp_png_path();
    std::fs::write(&path, png_bytes)
        .map_err(|e| OcrError::OcrFailed(format!("Failed to write temporary OCR image: {e}")))?;
    Ok(path)
}

fn tesseract_args(path: &std::path::Path, language: &str) -> Vec<String> {
    vec![
        path.to_string_lossy().into_owned(),
        "stdout".to_string(),
        "-l".to_string(),
        language.to_string(),
    ]
}

fn run_tesseract_command(
    command: &str,
    path: &std::path::Path,
    language: &str,
) -> Result<String, OcrError> {
    let output = Command::new(command)
        .args(tesseract_args(path, language))
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                OcrError::NoOcrEngine
            } else {
                OcrError::OcrFailed(format!("Failed to run tesseract: {e}"))
            }
        })?;

    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map(|text| text.trim().to_string())
            .map_err(|e| OcrError::OcrFailed(format!("Tesseract output was not UTF-8: {e}")));
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(OcrError::OcrFailed(if stderr.is_empty() {
        format!("Tesseract exited with status {}", output.status)
    } else {
        stderr
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tesseract_args_include_stdout_and_language() {
        let path = std::path::Path::new("/tmp/demo.png");
        assert_eq!(
            tesseract_args(path, "eng+chi_sim"),
            vec!["/tmp/demo.png", "stdout", "-l", "eng+chi_sim"]
        );
    }

    #[test]
    fn test_ocr_language_defaults_to_english_and_simplified_chinese() {
        std::env::remove_var(OCR_LANG_ENV);
        assert_eq!(ocr_language(), DEFAULT_TESSERACT_LANG);
    }

    #[test]
    fn test_linux_ocr_service_rejects_invalid_image_before_running_tesseract() {
        let svc = OcrService {
            command: "tesseract".to_string(),
            language: "eng".to_string(),
        };
        let result = svc.recognize(&[]);
        assert!(matches!(result, Err(OcrError::ImageError(_))));
    }

    #[test]
    fn test_linux_ocr_service_error_message_mentions_linux() {
        let msg = OcrError::NoOcrEngine.to_string();
        let lowered = msg.to_lowercase();
        assert!(
            lowered.contains("tesseract") || lowered.contains("ocr engine"),
            "Error message should mention tesseract or OCR engine, got {:?}",
            msg
        );
    }
}
