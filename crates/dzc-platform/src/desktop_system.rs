use std::io;

pub fn supported() -> bool { cfg!(target_os = "windows") }

pub fn win_v_optimized() -> io::Result<bool> {
    #[cfg(target_os = "windows")]
    { crate::system_settings::win_v_optimized() }
    #[cfg(target_os = "linux")]
    { Err(io::ErrorKind::Unsupported.into()) }
}

pub fn set_clipboard_history(enabled: bool) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    { crate::system_settings::set_clipboard_history(enabled) }
    #[cfg(target_os = "linux")]
    { let _ = enabled; Err(io::ErrorKind::Unsupported.into()) }
}

pub struct WinVChange { token: String }
impl WinVChange {
    pub fn begin(enabled: bool) -> io::Result<(bool, Self)> {
        #[cfg(target_os = "windows")]
        {
            let (changed, token) = crate::system_settings::begin_win_v_change(enabled)?;
            Ok((changed, Self { token }))
        }
        #[cfg(target_os = "linux")]
        { let _ = enabled; Err(io::ErrorKind::Unsupported.into()) }
    }
    pub fn token(&self) -> &str { &self.token }
    pub fn finish(&mut self, rollback: bool) -> io::Result<bool> {
        #[cfg(target_os = "windows")]
        {
            let changed = crate::system_settings::finish_win_v_change(&self.token, rollback)?;
            self.token.clear();
            Ok(changed)
        }
        #[cfg(target_os = "linux")]
        { let _ = rollback; Err(io::ErrorKind::Unsupported.into()) }
    }
}
impl Drop for WinVChange {
    fn drop(&mut self) {
        if !self.token.is_empty() {
            if let Err(error) = self.finish(true) { eprintln!("Windows setting rollback on disconnect failed: {error}"); }
        }
    }
}
