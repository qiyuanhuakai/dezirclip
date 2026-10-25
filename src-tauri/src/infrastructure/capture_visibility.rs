/// Confirm the native hide before acquiring a new selector frame. Windows also
/// waits for DWM to finish the submitted composition work. Other platforms keep
/// the freeze-before-show path; native compositor retirement is not inferred
/// from an X11 round trip or a timer.
pub fn confirm_hidden_selector(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.is_visible().map_err(|error| error.to_string())? {
        return Err("RegionSelectorStillVisible".into());
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Graphics::Dwm::DwmFlush;
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        let hwnd = window.hwnd().map_err(|error| error.to_string())?;
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                return Err("RegionSelectorStillVisible".into());
            }
            DwmFlush().map_err(|_| "RegionCaptureCompositionUnavailable".to_string())?;
            if IsWindowVisible(hwnd).as_bool() {
                return Err("RegionSelectorStillVisible".into());
            }
        }
    }
    Ok(())
}
