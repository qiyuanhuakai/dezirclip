use std::{marker::PhantomData, rc::Rc};
use windows::{core::w, Win32::{
    Foundation::HWND,
    System::DataExchange::{GetOpenClipboardWindow, OpenClipboard},
    UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE},
}};

/// A read transaction has a real window identity and stays on its creating thread.
pub(super) struct ClipboardReadGuard {
    window: HWND,
    _thread: PhantomData<Rc<()>>,
}

impl ClipboardReadGuard {
    pub(super) fn open() -> windows::core::Result<Self> {
        unsafe {
            let window = CreateWindowExW(WINDOW_EX_STYLE::default(), w!("STATIC"), w!("DezirClip clipboard reader"),
                WINDOW_STYLE::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None)?;
            if let Err(error) = OpenClipboard(Some(window)) {
                let _ = DestroyWindow(window);
                return Err(error);
            }
            let guard = Self { window, _thread: PhantomData };
            if GetOpenClipboardWindow()? != window {
                return Err(windows::core::Error::new(windows::core::HRESULT(0x80004005u32 as i32), "ClipboardReadIdentityChanged"));
            }
            Ok(guard)
        }
    }
}

impl Drop for ClipboardReadGuard {
    fn drop(&mut self) {
        unsafe {
            // Retain the anonymous-token protection used by the previous text backend.
            let _ = clipboard_win::raw::close();
            let _ = DestroyWindow(self.window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_window_owns_the_lock_until_drop() {
        if std::env::var("DZC_TEST_CLIPBOARD_READER").as_deref() != Ok("1") { return; }
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(std::env::var("RUNNER_ENVIRONMENT").as_deref(), Ok("github-hosted"));
        let guard = ClipboardReadGuard::open().unwrap();
        assert_eq!(unsafe { GetOpenClipboardWindow().unwrap() }, guard.window);
        assert!(ClipboardReadGuard::open().is_err(), "A nested transaction replaced the reader lock");
        std::thread::spawn(|| unsafe {
            let opened = OpenClipboard(None).is_ok();
            if opened { let _ = clipboard_win::raw::close(); }
            assert!(!opened, "Another thread replaced the reader lock");
        }).join().unwrap();
        assert_eq!(unsafe { GetOpenClipboardWindow().unwrap() }, guard.window);
        drop(guard);
        let guard = ClipboardReadGuard::open().unwrap();
        let text = "CI clipboard reader 中文 😀\r\nsecond line";
        let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe {
            use windows::Win32::{
                Foundation::{GlobalFree, HANDLE},
                System::{DataExchange::{EmptyClipboard, GetClipboardSequenceNumber, SetClipboardData},
                    Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE}},
            };
            EmptyClipboard().unwrap();
            let memory = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2).unwrap();
            let data = GlobalLock(memory);
            if data.is_null() {
                let _ = GlobalFree(Some(memory));
                panic!("Synthetic clipboard allocation could not be locked");
            }
            std::ptr::copy_nonoverlapping(units.as_ptr(), data.cast::<u16>(), units.len());
            let _ = GlobalUnlock(memory);
            if let Err(error) = SetClipboardData(13, Some(HANDLE(memory.0))) {
                let _ = GlobalFree(Some(memory));
                panic!("Synthetic clipboard publication failed: {error}");
            }
            drop(guard);
            let sequence = GetClipboardSequenceNumber();
            assert_eq!(super::super::windows_clipboard::get_clipboard_text().as_deref(), Some(text));
            assert_eq!(GetClipboardSequenceNumber(), sequence, "Read changed the clipboard selection");
            let _cleanup = ClipboardReadGuard::open().unwrap();
            EmptyClipboard().unwrap();
        }
    }
}
