//! Eager public image publication with a real, thread-bound clipboard owner.
use std::{marker::PhantomData, rc::Rc, time::Duration};
use windows::{core::w, Win32::{
    Foundation::{GlobalFree, HANDLE, HWND},
    System::{DataExchange::{EmptyClipboard, GetOpenClipboardWindow, OpenClipboard, RegisterClipboardFormatW, SetClipboardData},
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE}},
    UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE},
}};

struct Window(HWND, PhantomData<Rc<()>>);
impl Drop for Window {
    fn drop(&mut self) { unsafe { let _ = DestroyWindow(self.0); } }
}
struct ClipboardLock;
impl Drop for ClipboardLock {
    fn drop(&mut self) { let _ = clipboard_win::raw::close(); }
}
fn native_failure(stage: &str, error: windows::core::Error) -> String {
    eprintln!("Native image publication failed: {stage}, HRESULT={:08x}", error.code().0 as u32);
    "ClipboardWriteUnavailable".into()
}

fn representations(image: image::RgbaImage) -> Result<(Vec<u8>, Vec<u8>), String> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32
        || image.len() > 256 * 1024 * 1024 - 124 {
        return Err("ClipboardImageRejected".into());
    }
    let mut png = Vec::new();
    use image::{codecs::png::{CompressionType, FilterType, PngEncoder}, ImageEncoder};
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
        .write_image(&image, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|_| "ClipboardImageRejected")?;
    let mut dib = vec![0u8; 124];
    for (offset, value) in [(0, 124u32), (4, width), (8, height), (16, 3), (20, image.len() as u32),
        (40, 0x00ff0000), (44, 0x0000ff00), (48, 0x000000ff), (52, 0xff000000), (56, 0x73524742), (108, 4)] {
        dib[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    // Positive-height bottom-up BGRA keeps the existing Office-compatible view.
    for row in image.rows().rev() {
        for pixel in row { dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]); }
    }
    Ok((png, dib))
}

fn publish(format: u32, bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|error| native_failure("allocate", error))?;
        let target = GlobalLock(memory);
        if target.is_null() {
            let error = windows::core::Error::from_thread();
            let _ = GlobalFree(Some(memory));
            return Err(native_failure("lock-memory", error));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(memory);
        if let Err(error) = SetClipboardData(format, Some(HANDLE(memory.0))) {
            let _ = GlobalFree(Some(memory));
            return Err(native_failure("set-data", error));
        }
        // Windows owns the allocation after successful publication.
    }
    Ok(())
}

pub(super) fn write(image: image::RgbaImage) -> Result<(), String> {
    // All encoding and row conversion precedes the system-wide clipboard lock.
    let (png, dib) = representations(image)?;
    let format = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    if format == 0 { return Err(native_failure("register-png", windows::core::Error::from_thread())); }
    write_formats(vec![(format, png), (17, dib)])
}

pub(super) fn write_formats(formats: Vec<(u32, Vec<u8>)>) -> Result<(), String> {
    let marker = crate::capture::windows_echo::format()?;
    let window = Window(unsafe {
        CreateWindowExW(WINDOW_EX_STYLE::default(), w!("STATIC"), w!("DezirClip image clipboard"),
            WINDOW_STYLE::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None)
    }.map_err(|error| native_failure("create-owner", error))?, PhantomData);
    let mut attempt = 0;
    let _clipboard = loop {
        match unsafe { OpenClipboard(Some(window.0)) } {
            Ok(()) => break ClipboardLock,
            Err(error) if attempt == 5 => return Err(native_failure("open", error)),
            Err(_) => { attempt += 1; std::thread::sleep(Duration::from_millis(5)); }
        }
    };
    if unsafe { GetOpenClipboardWindow() }.ok() != Some(window.0) {
        return Err("ClipboardWriteIdentityChanged".into());
    }
    unsafe { EmptyClipboard() }.map_err(|error| native_failure("empty", error))?;
    // No delayed rendering survives this transaction. The marker and payload
    // become visible together, before any capture notification can read them.
    let result = formats.iter().try_for_each(|(format, bytes)| publish(*format, bytes))
        .and_then(|()| publish(marker, &[1]));
    if result.is_err() {
        // The same lock still excludes later owners; do not leave a partial image.
        let _ = unsafe { EmptyClipboard() };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_and_bottom_up_dib_preserve_pixels_and_alpha() {
        let pixels = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let (png, dib) = representations(image::RgbaImage::from_raw(2, 2, pixels.clone()).unwrap()).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert_eq!(decoded.into_raw(), pixels);
        assert_eq!(&dib[8..12], &2i32.to_le_bytes());
        assert_eq!(&dib[124..], &[11, 10, 9, 12, 15, 14, 13, 16, 3, 2, 1, 4, 7, 6, 5, 8]);
        assert_eq!(&dib[52..56], &0xff000000u32.to_le_bytes());
    }
    #[test]
    fn empty_images_are_rejected_before_opening_the_clipboard() {
        assert!(representations(image::RgbaImage::new(0, 0)).is_err());
    }
    #[test]
    fn published_image_survives_transaction_and_owner_destruction() {
        if std::env::var("DZC_TEST_CLIPBOARD_READER").as_deref() != Ok("1") { return; }
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(std::env::var("RUNNER_ENVIRONMENT").as_deref(), Ok("github-hosted"));
        let pixels = vec![11, 22, 33, 44, 55, 66, 77, 88];
        write(image::RgbaImage::from_raw(1, 2, pixels.clone()).unwrap()).unwrap();
        let mut clipboard = arboard::Clipboard::new().unwrap();
        let image = clipboard.get_image().unwrap();
        assert_eq!((image.width, image.height), (1, 2));
        assert_eq!(image.bytes.as_ref(), pixels.as_slice());
        {
            let _clipboard = clipboard_win::Clipboard::new_attempts(10).unwrap();
            let mut dib = Vec::new();
            clipboard_win::raw::get_vec(17, &mut dib).unwrap();
            assert_eq!(&dib[124..], &[77, 66, 55, 88, 33, 22, 11, 44]);
        }
        clipboard.clear().unwrap();
    }
}
