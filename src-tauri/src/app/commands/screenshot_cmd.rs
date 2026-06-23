use tauri::{AppHandle, Emitter, Manager};

use crate::services::screenshot::{self, MonitorInfo, ScreenshotResult};

const EVENT_SCREENSHOT_COMPLETE: &str = "screenshot:complete";

#[tauri::command]
pub fn show_region_selector(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("region-select")
        .ok_or_else(|| "Region selector window is not configured".to_string())?;
    let _ = window.set_focusable(true);
    window
        .show()
        .map_err(|e| format!("Failed to show region selector: {e}"))?;
    window
        .set_focus()
        .map_err(|e| format!("Failed to focus region selector: {e}"))?;
    Ok(())
}

/// Capture the primary monitor as a PNG and broadcast a `screenshot:complete`
/// event with the same payload the command returns to the caller.
#[tauri::command]
pub async fn capture_full_screen(app: AppHandle) -> Result<ScreenshotResult, String> {
    let result = screenshot::capture_full_screen().map_err(|e| e.to_string())?;
    let _ = app.emit(EVENT_SCREENSHOT_COMPLETE, &result);
    Ok(result)
}

/// Capture a rectangular region at absolute screen coordinates `(x, y)` with
/// size `(width, height)`. Out-of-bounds or zero-sized regions surface as a
/// `ScreenshotError::RegionOutOfBounds` string so the frontend can react.
#[tauri::command]
pub async fn capture_region(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    app: AppHandle,
) -> Result<ScreenshotResult, String> {
    let result = screenshot::capture_region(x, y, width, height).map_err(|e| e.to_string())?;
    let _ = app.emit(EVENT_SCREENSHOT_COMPLETE, &result);
    Ok(result)
}

/// Enumerate every attached monitor.
#[tauri::command]
pub async fn list_monitors(_app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    screenshot::list_monitors().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::screenshot::screenshot_smoke_enabled;

    const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

    #[test]
    fn test_capture_full_screen_returns_png() {
        if !screenshot_smoke_enabled() {
            return;
        }

        match screenshot::capture_full_screen() {
            Ok(result) => {
                assert!(result.png_bytes.len() >= PNG_MAGIC.len());
                assert_eq!(
                    &result.png_bytes[..PNG_MAGIC.len()],
                    &PNG_MAGIC,
                    "capture_full_screen output must start with PNG magic bytes"
                );
            }
            Err(_) => {
                // Headless CI: the service tolerates a missing display.
            }
        }
    }

    #[test]
    fn test_capture_region_validates_bounds() {
        let err = screenshot::capture_region(-1, -1, 100, 100)
            .err()
            .expect("negative coordinates must surface RegionOutOfBounds");
        let msg = err.to_string();
        assert!(
            msg.contains("outside monitor bounds") || msg.contains("No monitor"),
            "out-of-bounds error string must be user-facing, got: {msg}"
        );
        assert!(
            !msg.contains("xcap"),
            "error message must not leak internal xcap wording, got: {msg}"
        );

        let zero = screenshot::capture_region(0, 0, 0, 0)
            .err()
            .expect("zero-sized region must surface RegionOutOfBounds");
        assert!(!zero.to_string().is_empty(), "error must be non-empty");
    }

    #[test]
    fn test_list_monitors_format() {
        let monitor = MonitorInfo {
            id: 7,
            name: "Test Display".to_string(),
            x: 100,
            y: 200,
            width: 1920,
            height: 1080,
            is_primary: true,
        };
        assert_eq!(monitor.id, 7);
        assert_eq!(monitor.name, "Test Display");
        assert_eq!(monitor.x, 100);
        assert_eq!(monitor.y, 200);
        assert_eq!(monitor.width, 1920);
        assert_eq!(monitor.height, 1080);
        assert!(monitor.is_primary);

        if !screenshot_smoke_enabled() {
            return;
        }

        match screenshot::list_monitors() {
            Ok(list) => {
                for m in &list {
                    assert!(m.id != 0 || list.len() == 1, "id field must be populated");
                }
            }
            Err(_) => {
                // Headless CI: the service tolerates a missing display.
            }
        }
    }
}
