#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;
#[cfg(target_os = "windows")]
mod windows_icon;
#[cfg(target_os = "windows")]
pub use windows_icon::{get_executable_icon, get_source_app_icon};

pub fn open_url(handler: &Handler, value: &str) -> std::io::Result<()> {
    let value = value.trim();
    let normalized = if value.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("www.")) {
        format!("https://{value}")
    } else {
        value.to_owned()
    };
    let url = url::Url::parse(&normalized).map_err(std::io::Error::other)?;
    if !matches!(url.scheme(), "https" | "http" | "ftp") || !url.has_host() {
        return Err(std::io::Error::other("InvalidExternalUrl"));
    }
    launch(handler, url.as_str(), true)
}

pub fn open_file(handler: &Handler, path: &str) -> std::io::Result<()> {
    if !std::path::Path::new(path).is_absolute() || !std::path::Path::new(path).is_file() {
        return Err(std::io::Error::other("ExternalFileUnavailable"));
    }
    launch(handler, path, false)
}
