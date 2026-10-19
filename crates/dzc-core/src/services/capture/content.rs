use base64::{engine::general_purpose, Engine as _};
use regex::Regex;
use std::sync::OnceLock;
use urlencoding::decode;

pub fn detect_content_type(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("http") || trimmed.starts_with("www.") {
        return "url".to_string();
    }

    let mut score = 0;
    let keywords = [
        "import ",
        "const ",
        "let ",
        "var ",
        "function ",
        "class ",
        "pub fn ",
        "impl ",
        "#include",
        "package ",
        "interface ",
        "namespace ",
        "void ",
        "return ",
        "if (",
        "for (",
        "while (",
        "=>",
    ];

    for k in keywords {
        if text.contains(k) {
            score += 1;
        }
    }

    if text.contains(";") {
        score += 1;
    }
    if text.contains("{") && text.contains("}") {
        score += 1;
    }
    if text.contains("</") && text.contains(">") {
        score += 2;
    }

    if score >= 2 {
        return "code".to_string();
    }

    if trimmed.starts_with("{")
        && trimmed.ends_with("}")
        && text.contains(":")
        && text.contains("\"")
    {
        return "code".to_string();
    }

    "text".to_string()
}

pub fn contains_sensitive_info(text: &str, kinds: &[String], custom_rules: &[String]) -> bool {
    static PHONE_RE: OnceLock<Regex> = OnceLock::new();
    static IDCARD_RE: OnceLock<Regex> = OnceLock::new();
    static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
    static SECRET_RE: OnceLock<Regex> = OnceLock::new();

    if text.len() > 5000 || text.starts_with("data:") {
        return false;
    }

    let has_kind = |k: &str| kinds.iter().any(|t| t == k);

    if has_kind("phone") {
        let re = PHONE_RE.get_or_init(|| {
            Regex::new(r"(?:\+?86)?[-\s\(]*1[3-9]\d{1}[-\s\)]*\d{4}[-\s]*\d{4}").unwrap()
        });
        if re.is_match(text) {
            return true;
        }
    }
    if has_kind("idcard") {
        let re = IDCARD_RE.get_or_init(|| {
            Regex::new(
                r"\b[1-9]\d{5}[1-9]\d{3}((0\d)|(1[0-2]))(([0|1|2]\d)|3[0-1])\d{3}([0-9Xx])\b",
            )
            .unwrap()
        });
        if re.is_match(text) {
            return true;
        }
    }
    if has_kind("email") {
        let re = EMAIL_RE
            .get_or_init(|| Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());
        if re.is_match(text) {
            return true;
        }
    }
    if has_kind("secret") {
        let re = SECRET_RE.get_or_init(|| Regex::new(r"(?ix)((?:sk|pk|ghp|gho|github_pat|AIza|AKIA|ya29)[-_][\w\-]{20,}|(?:password|secret|api[_-]?key|access[_-]?key|token|bearer)[\s:=]+[\w\-]{16,})").unwrap());
        if re.is_match(text) {
            return true;
        }
    }
    if has_kind("password") {
        if text.len() >= 8 && text.len() <= 64 && !text.contains(' ') && !text.contains('\n') {
            let has_upper = text.chars().any(|c| c.is_uppercase());
            let has_lower = text.chars().any(|c| c.is_lowercase());
            let has_digit = text.chars().any(|c| c.is_numeric());
            let has_special = text.chars().any(|c| !c.is_alphanumeric());
            if has_upper && has_lower && has_digit && has_special {
                return true;
            }
        }
    }

    for rule in custom_rules {
        if let Ok(re) = Regex::new(rule) {
            if re.is_match(text) {
                return true;
            }
        }
    }
    false
}

pub fn embed_local_images(html: &str) -> Result<String, String> {
    const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
    const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
    const MAX_IMAGES: usize = 64;
    let mut input_bytes = 0usize;
    let mut count = 0usize;
    let mut failure = None;
    let re = match Regex::new(r#"(<img\s+[^>]*src=["'])([^"']+)(["'][^>]*>)"#) {
        Ok(r) => r,
        Err(_) => return Err("AttachmentReadRejected".into()),
    };

    let rewritten = re
        .replace_all(html, |caps: &regex::Captures| {
            if failure.is_some() {
                return caps[0].to_owned();
            }
            let prefix = &caps[1];
            let src = &caps[2];
            let suffix = &caps[3];

            let is_local = src.starts_with("file://")
                || (src.len() > 2
                    && src.chars().nth(1) == Some(':')
                    && (src.chars().nth(2) == Some('\\') || src.chars().nth(2) == Some('/')));

            if is_local {
                let path_str = if src.starts_with("file://") {
                    let raw_path = src.trim_start_matches("file://");
                    if raw_path.starts_with('/') && raw_path.chars().nth(2) == Some(':') {
                        &raw_path[1..]
                    } else {
                        raw_path
                    }
                } else {
                    src
                };

                let decoded_path = decode(path_str)
                    .map(|p| p.into_owned())
                    .unwrap_or(path_str.to_string());
                let clean_path = decoded_path
                    .split('?')
                    .next()
                    .unwrap_or(&decoded_path)
                    .split('#')
                    .next()
                    .unwrap_or(&decoded_path);

                let path = std::path::Path::new(clean_path);
                count += 1;
                if count > MAX_IMAGES {
                    failure = Some("AttachmentBudgetExceeded".into());
                    return caps[0].to_owned();
                }
                let maximum = MAX_IMAGE_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(input_bytes));
                match crate::infrastructure::local_file::read_bounded_regular(path, maximum) {
                    Ok(data) => {
                        input_bytes += data.len();
                        let ext = path
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("png")
                            .to_lowercase();
                        let mime = match ext.as_str() {
                            "jpg" | "jpeg" => "image/jpeg",
                            "gif" => "image/gif",
                            "webp" => "image/webp",
                            "bmp" => "image/bmp",
                            "svg" => "image/svg+xml",
                            _ => "image/png",
                        };
                        let b64 = general_purpose::STANDARD.encode(&data);
                        return format!(
                            "{}{}{}",
                            prefix,
                            format!("data:{};base64,{}", mime, b64),
                            suffix
                        );
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound
                            || error.kind() == std::io::ErrorKind::PermissionDenied => {}
                    Err(error) => {
                        failure = Some(
                            if error.kind() == std::io::ErrorKind::InvalidData {
                                "AttachmentBudgetExceeded"
                            } else {
                                "AttachmentReadRejected"
                            }
                            .into(),
                        )
                    }
                }
            }

            format!("{}{}{}", prefix, src, suffix)
        })
        .to_string();
    match failure {
        Some(error) => Err(error),
        None => Ok(rewritten),
    }
}
