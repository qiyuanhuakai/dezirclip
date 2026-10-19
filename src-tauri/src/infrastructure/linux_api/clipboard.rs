pub use dzc_platform::capture::x11_clipboard::{
    get_clipboard_files, get_clipboard_html, get_clipboard_image, get_clipboard_raw_format,
    get_clipboard_sequence_number, set_clipboard_files, set_clipboard_image_with_formats,
    ImageData,
};

pub fn set_clipboard_text_and_html(
    text: String,
    html: Option<String>,
) -> crate::error::AppResult<()> {
    dzc_platform::capture::x11_clipboard::set_clipboard_text_and_html(text, html)
        .map_err(crate::error::AppError::Internal)
}
