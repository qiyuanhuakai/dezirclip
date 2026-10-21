use dzc_platform::{
    capture::x11_clipboard as x11,
    native_clipboard::{self, Payload},
};
pub use x11::ImageData;

pub fn get_clipboard_sequence_number() -> u32 {
    if dzc_platform::runtime::is_native_wayland() {
        dzc_platform::wayland::WaylandClipboardBackend::snapshot()
            .map(|s| s.sequence() as u32)
            .unwrap_or(0)
    } else {
        x11::get_clipboard_sequence_number()
    }
}
pub fn get_clipboard_raw_format(name: &str) -> Option<Vec<u8>> {
    if dzc_platform::runtime::is_native_wayland() {
        native_clipboard::read(name).ok()
    } else {
        x11::get_clipboard_raw_format(name)
    }
}
pub fn get_clipboard_html() -> Option<String> {
    if dzc_platform::runtime::is_native_wayland() {
        String::from_utf8(native_clipboard::read("text/html").ok()?).ok()
    } else {
        x11::get_clipboard_html()
    }
}
pub fn get_clipboard_files() -> Option<Vec<String>> {
    if !dzc_platform::runtime::is_native_wayland() {
        return x11::get_clipboard_files();
    }
    let bytes = native_clipboard::read("text/uri-list").ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let paths = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let url = tauri::Url::parse(line).ok()?;
            url.to_file_path()
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .collect::<Option<Vec<_>>>()?;
    (!paths.is_empty()).then_some(paths)
}
pub fn get_clipboard_image() -> Option<ImageData> {
    if !dzc_platform::runtime::is_native_wayland() {
        return x11::get_clipboard_image();
    }
    let image = image::load_from_memory(&native_clipboard::read("image/png").ok()?)
        .ok()?
        .into_rgba8();
    Some(ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: image.into_raw(),
    })
}
pub fn set_clipboard_files(paths: Vec<String>) -> Result<(), String> {
    if dzc_platform::runtime::is_native_wayland() {
        native_clipboard::write_embedded(Payload::Files(paths))
    } else {
        x11::set_clipboard_files(paths)
    }
}
pub fn set_clipboard_image_with_formats(data: ImageData) -> Result<(), String> {
    if !dzc_platform::runtime::is_native_wayland() {
        return x11::set_clipboard_image_with_formats(data);
    }
    let width = u32::try_from(data.width).map_err(|_| "ClipboardImageInvalid")?;
    let height = u32::try_from(data.height).map_err(|_| "ClipboardImageInvalid")?;
    if data.bytes.len() > dzc_platform::wayland::MAX_TRANSFER_BYTES {
        return Err("WaylandTransferBudgetExceeded".into());
    }
    let image =
        image::RgbaImage::from_raw(width, height, data.bytes).ok_or("ClipboardImageInvalid")?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|_| "ClipboardImageInvalid")?;
    native_clipboard::write_embedded(Payload::Image(bytes.into_inner()))
}
pub fn set_clipboard_text_and_html(
    text: String,
    html: Option<String>,
) -> crate::error::AppResult<()> {
    let result = if dzc_platform::runtime::is_native_wayland() {
        native_clipboard::write_embedded(match html {
            Some(html) => Payload::Html { html, text },
            None => Payload::Text(text),
        })
    } else {
        x11::set_clipboard_text_and_html(text, html)
    };
    result.map_err(crate::error::AppError::Internal)
}
