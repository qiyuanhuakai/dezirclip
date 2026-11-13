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

pub fn builtin_source_icon(source: &str) -> Option<String> {
    if !matches!(source, "DezirClip Screenshot" | "DezirClip File Drop" | "DezirClip Derived" | "DezirClip") { return None; }
    use base64::Engine;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(
        include_bytes!("dezirclip.png"))))
}

#[cfg(test)]
mod builtin_icon_tests {
    #[test]
    fn generated_sources_share_a_real_embedded_application_icon() {
        use base64::Engine;
        for source in ["DezirClip Screenshot", "DezirClip File Drop", "DezirClip Derived"] {
            let icon = super::builtin_source_icon(source).unwrap();
            let bytes = base64::engine::general_purpose::STANDARD.decode(icon.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
            let image = image::load_from_memory(&bytes).unwrap().into_rgba8();
            assert!(image.pixels().any(|pixel| pixel[3] > 0));
        }
        assert!(super::builtin_source_icon("other application").is_none());
    }
}

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
