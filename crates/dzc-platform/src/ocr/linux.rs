use std::{io::{Read,Write}, process::{Command,Stdio},time::{Duration,Instant}};

use super::OcrError;

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
        let available = invoke(&command, &["--version".into()], Vec::new(), Duration::from_secs(5)).is_ok();
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
        self.run_tesseract(png_bytes)
    }

    fn run_tesseract(&self, png_bytes: &[u8]) -> Result<String, OcrError> {
        match run_tesseract_command(&self.command, png_bytes, &self.language) {
            Ok(text) => Ok(text),
            Err(err) if self.language != "eng" => run_tesseract_command(&self.command, png_bytes, "eng")
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

fn tesseract_args(language: &str) -> Vec<String> {
    vec![
        "stdin".to_owned(),
        "stdout".to_string(),
        "-l".to_string(),
        language.to_string(),
    ]
}

fn run_tesseract_command(
    command: &str,
    png_bytes: &[u8],
    language: &str,
) -> Result<String, OcrError> {
    let bytes=invoke(command,&tesseract_args(language),png_bytes.to_vec(),Duration::from_secs(30))?;
    String::from_utf8(bytes).map(|text|text.trim().to_owned()).map_err(|_|failure())
}

fn failure() -> OcrError {OcrError::OcrFailed("Native OCR failed or exceeded its resource deadline".into())}
fn bounded(mut reader: impl Read, maximum:u64)->std::io::Result<Vec<u8>> {
    let mut bytes=Vec::new();
    (&mut reader).take(maximum+1).read_to_end(&mut bytes)?;
    if bytes.len() as u64>maximum {return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));}
    Ok(bytes)
}
fn invoke(command:&str,arguments:&[String],bytes:Vec<u8>,timeout:Duration)->Result<Vec<u8>,OcrError> {
    let mut child=Command::new(command).args(arguments).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .map_err(|error|if error.kind()==std::io::ErrorKind::NotFound {OcrError::NoOcrEngine} else {failure()})?;
    let mut input=child.stdin.take().ok_or_else(failure)?;
    let output=child.stdout.take().ok_or_else(failure)?;
    let diagnostics=child.stderr.take().ok_or_else(failure)?;
    let writer=std::thread::spawn(move||input.write_all(&bytes));
    let reader=std::thread::spawn(move||bounded(output,8*1024*1024));
    let errors=std::thread::spawn(move||bounded(diagnostics,64*1024));
    let deadline=Instant::now()+timeout;
    let mut expired=false;
    let status=loop {
        match child.try_wait() {
            Ok(Some(status))=>break Ok(status),
            Err(error)=>{let _=child.kill();let _=child.wait();break Err(error);},
            Ok(None) if Instant::now()>=deadline=>{expired=true;let _=child.kill();break child.wait();},
            Ok(None)=>std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let input=writer.join().map_err(|_|failure())?;
    let output=reader.join().map_err(|_|failure())?.map_err(|_|failure())?;
    let diagnostics=errors.join().map_err(|_|failure())?.map_err(|_|failure())?;
    drop(diagnostics);
    if expired || !status.map_err(|_|failure())?.success() || input.is_err() {return Err(failure());}
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tesseract_args_include_stdout_and_language() {
        assert_eq!(
            tesseract_args("eng+chi_sim"),
            vec!["stdin", "stdout", "-l", "eng+chi_sim"]
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
