use std::{marker::PhantomData, rc::Rc};
use windows::{core::w, Win32::{
    Foundation::HWND,
    System::DataExchange::{CloseClipboard, GetOpenClipboardWindow, OpenClipboard},
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
            let _ = CloseClipboard();
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
            if opened { let _ = CloseClipboard(); }
            assert!(!opened, "Another thread replaced the reader lock");
        }).join().unwrap();
        assert_eq!(unsafe { GetOpenClipboardWindow().unwrap() }, guard.window);
        drop(guard);
        drop(ClipboardReadGuard::open().unwrap());
    }
}
