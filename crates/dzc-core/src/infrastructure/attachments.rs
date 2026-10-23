//! Stored attachment decoding and atomic backup file replacement.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn path_from_uri(value: &str) -> Result<String, String> {
    let Some(uri) = value.strip_prefix("file://") else {
        return Ok(value.to_owned());
    };
    let raw = if uri.starts_with('/') || uri.as_bytes().get(1) == Some(&b':') {
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
        .map_err(|error| error.to_string())?
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
        STANDARD.decode(payload).map_err(|error| error.to_string())
    } else {
        std::fs::read(path_from_uri(content)?).map_err(|error| format!("read image: {error}"))
    }
}

pub fn resolved_path(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path.canonicalize().map_err(|error| error.to_string());
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    let mut base = absolute.as_path();
    let mut suffix = Vec::new();
    while !base.exists() {
        suffix.push(base.file_name().ok_or("invalid backup path")?.to_owned());
        base = base.parent().ok_or("invalid backup path")?;
    }
    let mut result = base.canonicalize().map_err(|error| error.to_string())?;
    for component in suffix.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
}

pub fn write_backup(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".dezirclip-backup-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

/// A capture can determine stable HTML and deduplication keys without creating files.
pub struct HtmlAttachmentPlan {
    html: String,
    files: Vec<(PathBuf, Vec<u8>)>,
    input_bytes: usize,
}

#[derive(Clone, Copy)]
struct HtmlAttachmentBudget { image: usize, total: usize, count: usize }
const HTML_ATTACHMENT_BUDGET: HtmlAttachmentBudget = HtmlAttachmentBudget {
    image: 16 * 1024 * 1024, total: 32 * 1024 * 1024, count: 64,
};

impl HtmlAttachmentPlan {
    pub fn html(&self) -> &str {
        &self.html
    }

    fn add_image(&mut self, directory: &Path, bytes: Vec<u8>, extension: &str, budget: HtmlAttachmentBudget) -> Result<PathBuf, String> {
        if bytes.len() > budget.image || bytes.len() > budget.total.saturating_sub(self.input_bytes) {
            return Err("AttachmentBudgetExceeded".into());
        }
        self.input_bytes += bytes.len();
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        let path = directory.join(format!("img_{:x}.{extension}", hasher.finish()));
        if !self.files.iter().any(|(existing, _)| existing == &path) {
            self.files.push((path.clone(), bytes));
        }
        Ok(path)
    }

    /// Restore planned RAM images without ever creating their plaintext files.
    pub fn inline_html(&self) -> Result<String, String> {
        let mut html = self.html.clone();
        for (path, bytes) in &self.files {
            let mime = image::guess_format(bytes).map_err(|_| "AttachmentPersistenceRejected")?.to_mime_type();
            let replacement = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
            let raw = path.to_string_lossy();
            let normalized = raw.replace('\\', "/");
            let uri = if normalized.starts_with('/') { format!("file://{normalized}") } else { format!("file:///{normalized}") };
            html = html.replace(&uri, &replacement).replace(raw.as_ref(), &replacement);

        }
        crate::infrastructure::protected_resources::normalize_html(&html)
    }

    pub fn materialize(&self) -> Result<CreatedAttachments, String> {
        let mut created = CreatedAttachments { paths: Vec::new() };
        for (path, bytes) in &self.files {
            let parent = path.parent().ok_or("AttachmentPersistenceFailed")?;
            std::fs::create_dir_all(parent).map_err(|_| "AttachmentPersistenceFailed")?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = match options.open(path) {
                Ok(file) => file,
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_file() =>
                {
                    continue
                }
                Err(_) => return Err("AttachmentPersistenceFailed".to_string()),
            };
            created.paths.push(path.clone());
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| "AttachmentPersistenceFailed")?;
        }
        Ok(created)
    }
}

/// Only files created by this capture are removed when its transaction fails.
pub struct CreatedAttachments {
    paths: Vec<PathBuf>,
}

impl CreatedAttachments {
    pub fn keep(mut self) {
        self.paths.clear();
    }
}

impl Drop for CreatedAttachments {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn plan_html_attachments(html: &str, data_dir: &Path) -> Result<HtmlAttachmentPlan, String> {
    plan_html_attachments_with_budget(html, data_dir, HTML_ATTACHMENT_BUDGET)
}

fn plan_html_attachments_with_budget(html: &str, data_dir: &Path, budget: HtmlAttachmentBudget) -> Result<HtmlAttachmentPlan, String> {
    let directory = data_dir.join("attachments");
    let mut plan = HtmlAttachmentPlan {
        html: String::new(),
        files: Vec::new(),
        input_bytes: 0,
    };
    let mut count = 0;
    let mut failure = None;
    let pattern = regex::Regex::new(r#"(<img\s+[^>]*src=["'])([^"']+)(["'][^>]*>)"#)
        .expect("fixed image attribute expression");
    let rewritten = pattern
        .replace_all(html, |captures: &regex::Captures| {
            if failure.is_some() { return captures[0].to_owned(); }
            let src = &captures[2];
            let local = src.starts_with("file://")
                || (src.len() > 2
                    && src.as_bytes()[1] == b':'
                    && matches!(src.as_bytes()[2], b'\\' | b'/'));
            if !local {
                return captures[0].to_owned();
            }
            let raw = src.strip_prefix("file://").unwrap_or(src);
            let raw = if raw.starts_with('/') && raw.as_bytes().get(2) == Some(&b':') {
                &raw[1..]
            } else {
                raw
            };
            let decoded = urlencoding::decode(raw)
                .map(|path| path.into_owned())
                .unwrap_or_else(|_| raw.to_string());
            let clean = decoded.split(['?', '#']).next().unwrap_or(&decoded);
            let source = Path::new(clean);
            if source.starts_with(&directory) {
                return captures[0].to_owned();
            }
            count += 1;
            if count > budget.count { failure = Some("AttachmentBudgetExceeded".to_string()); return captures[0].to_owned(); }
            let maximum = budget.image.min(budget.total.saturating_sub(plan.input_bytes));
            let bytes = match crate::infrastructure::local_file::read_bounded_regular(source, maximum) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                    failure = Some("AttachmentBudgetExceeded".to_string()); return captures[0].to_owned();
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound || error.kind() == std::io::ErrorKind::PermissionDenied => return captures[0].to_owned(),
                Err(_) => { failure = Some("AttachmentReadRejected".to_string()); return captures[0].to_owned(); }
            };
            let extension = source
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("png")
                .to_lowercase();
            let path = match plan.add_image(&directory, bytes, &extension, budget) {
                Ok(path) => path,
                Err(error) => { failure = Some(error); return captures[0].to_owned(); }
            };
            let path = path.to_string_lossy().replace('\\', "/");
            let uri = if path.starts_with('/') {
                format!("file://{path}")
            } else {
                format!("file:///{path}")
            };
            format!("{}{uri}{}", &captures[1], &captures[3])
        })
        .into_owned();
    if let Some(error) = failure { return Err(error); }

    const PREFIX: &str = "<!--TIEZ_RICH_IMAGE:";
    const SUFFIX: &str = "-->";
    plan.html = rewritten;
    if let Some(start) = plan.html.rfind(PREFIX) {
        let payload_start = start + PREFIX.len();
        if let Some(end) = plan.html[payload_start..].find(SUFFIX) {
            let payload_end = payload_start + end;
            let payload = plan.html[payload_start..payload_end].trim();
            if payload.starts_with("data:image/") {
                let maximum = budget.image.min(budget.total.saturating_sub(plan.input_bytes));
                let encoded = payload.split_once(',').map(|(_, value)| value).unwrap_or("");
                if count >= budget.count || encoded.len() > maximum.saturating_add(2) / 3 * 4 {
                    return Err("AttachmentBudgetExceeded".into());
                }
                if let Some(bytes) = payload
                    .split_once(',')
                    .and_then(|(_, value)| STANDARD.decode(value).ok())
                {
                    let mut clean = plan.html[..start].to_owned();
                    clean.push_str(&plan.html[payload_end + SUFFIX.len()..]);
                    let clean = clean.trim();
                    let base = if clean.is_empty() {
                        plan.html.clone()
                    } else {
                        clean.to_owned()
                    };
                    let path = plan.add_image(&directory, bytes, "png", budget)?;
                    plan.html = format!(
                        "{}\n{PREFIX}{}{SUFFIX}",
                        base.trim_end(),
                        path.to_string_lossy()
                    );
                }
            }
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_planning_enforces_single_total_and_reference_limits_before_writes() {
        let root = std::env::temp_dir().join(format!("dezirclip-html-budget-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source.png");
        std::fs::write(&source, [1u8;8]).unwrap();
        let image = format!("<img src='file://{}'>", source.to_string_lossy().replace('\\', "/"));
        for (html,budget) in [
            (image.clone(), HtmlAttachmentBudget { image: 7, total: 100, count: 10 }),
            (image.repeat(2), HtmlAttachmentBudget { image: 8, total: 15, count: 10 }),
            (image.repeat(2), HtmlAttachmentBudget { image: 8, total: 100, count: 1 }),
            ("<!--TIEZ_RICH_IMAGE:data:image/png;base64,AQIDBAUGBwg=-->".into(), HtmlAttachmentBudget { image: 7, total: 100, count: 10 }),
        ] {
            assert_eq!(plan_html_attachments_with_budget(&html, &root, budget).err().unwrap(), "AttachmentBudgetExceeded");
            assert!(!root.join("attachments").exists());
        }
        let plan = plan_html_attachments_with_budget(&image, &root, HtmlAttachmentBudget { image: 8, total: 8, count: 1 }).unwrap();
        assert_eq!(plan.input_bytes, 8);
        assert_eq!(plan.files.len(), 1);
        let file = std::fs::File::create(&source).unwrap();
        file.set_len(HTML_ATTACHMENT_BUDGET.image as u64 + 1).unwrap();
        drop(file);
        assert_eq!(plan_html_attachments(&image, &root).err().unwrap(), "AttachmentBudgetExceeded");
        assert!(!root.join("attachments").exists());
        assert!(source.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn html_planning_never_creates_files_and_failed_materialization_preserves_shared_assets() {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-html-plan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let source = directory.join("source.PNG");
        std::fs::write(&source, b"original image").unwrap();
        let html = format!("<p>ordinary</p><img src='file://{}'><!--TIEZ_RICH_IMAGE:data:image/png;base64,b3RoZXI=-->", source.to_string_lossy().replace('\\', "/"));
        let plan = plan_html_attachments(&html, &directory).unwrap();
        assert_eq!(plan.files.len(), 2);
        assert!(!directory.join("attachments").exists());
        assert!(plan.html().contains("img_"));
        assert!(!plan.html().contains("base64,"));
        std::fs::create_dir(directory.join("attachments")).unwrap();
        let shared = &plan.files[0].0;
        std::fs::write(shared, b"shared content").unwrap();
        let blocked = &plan.files[1].0;
        std::fs::create_dir(blocked).unwrap();
        assert_eq!(
            plan.materialize().err().unwrap(),
            "AttachmentPersistenceFailed"
        );
        assert_eq!(std::fs::read(shared).unwrap(), b"shared content");
        assert_eq!(std::fs::read(&source).unwrap(), b"original image");
        std::fs::remove_dir(blocked).unwrap();
        let receipt = plan.materialize().unwrap();
        assert!(blocked.is_file());
        drop(receipt);
        assert!(!blocked.exists());
        assert!(shared.is_file());
        let receipt = plan.materialize().unwrap();
        receipt.keep();
        assert_eq!(std::fs::read(blocked).unwrap(), b"other");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn replacement_is_complete_and_failed_rename_preserves_existing_target() {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-backup-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let backup = directory.join("backup.json");
        std::fs::write(&backup, b"previous backup").unwrap();
        let content = "完整备份\r\n".repeat(2000);
        write_backup(&backup, content.as_bytes()).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), content.as_bytes());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let existing_directory = directory.join("keep");
        std::fs::create_dir(&existing_directory).unwrap();
        std::fs::write(existing_directory.join("original"), b"keep").unwrap();
        assert!(write_backup(&existing_directory, b"new backup").is_err());
        assert_eq!(
            std::fs::read(existing_directory.join("original")).unwrap(),
            b"keep"
        );
        assert!(!std::fs::read_dir(&directory).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".dezirclip-backup-")));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
