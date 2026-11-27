//! The Darwin helper owns pasteboard access on its process main thread.
use std::{io, marker::PhantomData, rc::Rc};

pub struct MainThreadClipboard {
    clipboard: arboard::Clipboard,
    _thread: PhantomData<Rc<()>>,
}

impl MainThreadClipboard {
    pub fn open() -> io::Result<Self> {
        // pthread_main_np identifies the original process thread independently
        // of Rust thread names or whichever executor calls the adapter.
        if unsafe { libc::pthread_main_np() } != 1 {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "Darwin clipboard requires the main thread"));
        }
        let clipboard = arboard::Clipboard::new().map_err(io::Error::other)?;
        Ok(Self { clipboard, _thread: PhantomData })
    }

    pub fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.clipboard.set_text(text).map_err(io::Error::other)
    }

    pub fn read_text(&mut self) -> io::Result<String> {
        self.clipboard.get_text().map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_cannot_open_the_darwin_clipboard() {
        let error = std::thread::spawn(|| MainThreadClipboard::open().err().unwrap().kind()).join().unwrap();
        assert_eq!(error, io::ErrorKind::Unsupported);
    }
}
