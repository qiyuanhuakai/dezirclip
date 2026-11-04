//! A real foreground window owned by the authenticated synthetic Desktop client.
use std::{
    io, thread,
    time::{Duration, Instant},
};

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(owner: isize, text: *const u16, title: *const u16, flags: u32) -> i32;
    fn FindWindowW(class: *const u16, title: *const u16) -> isize;
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(window: isize, pid: *mut u32) -> u32;
    fn AllowSetForegroundWindow(pid: u32) -> i32;
    fn PostMessageW(window: isize, msg: u32, w: usize, l: isize) -> i32;
}

pub struct Panel(isize);
impl Panel {
    pub fn show() -> io::Result<Self> {
        let title: Vec<_> = format!("DezirClip quick fixture {}", std::process::id())
            .encode_utf16()
            .chain([0])
            .collect();
        let caption = title.clone();
        thread::spawn(move || unsafe {
            MessageBoxW(0, caption.as_ptr(), caption.as_ptr(), 0x10000);
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
            if Self::owned(window) && unsafe { GetForegroundWindow() } == window {
                return Ok(Self(window));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other("SyntheticPanelFailedToAcquireFocus"));
            }
            thread::sleep(Duration::from_millis(8));
        }
    }
    fn owned(window: isize) -> bool {
        let mut pid = 0;
        window != 0
            && unsafe { GetWindowThreadProcessId(window, &mut pid) } != 0
            && pid == std::process::id()
    }
    fn owned_foreground(&self) -> bool {
        Self::owned(self.0) && unsafe { GetForegroundWindow() } == self.0
    }
    pub fn grant(&self, verified_service: u32) -> io::Result<()> {
        if verified_service == 0 || verified_service == u32::MAX || !self.owned_foreground() {
            return Err(io::Error::other("OwnedPanelAndOriginalServiceRequired"));
        }
        if unsafe { AllowSetForegroundWindow(verified_service) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
impl Drop for Panel {
    fn drop(&mut self) {
        if Self::owned(self.0) {
            unsafe {
                PostMessageW(self.0, 0x0010, 0, 0);
            }
        }
    }
}
