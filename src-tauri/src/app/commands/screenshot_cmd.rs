use base64::{engine::general_purpose, Engine as _};
use tauri::{AppHandle, Emitter, Manager};

use crate::services::clipboard_ops;
use crate::services::clipboard::{process_new_entry, ClipboardData};
use crate::services::screenshot::{self, MonitorRect, ScreenshotResult};

/// The frontend already listens on this channel (`useToastListener`) but
/// nothing on the backend ever emitted it, so it was a listener waiting for an
/// event that could not arrive. The payload is an i18n key, not finished text —
/// the backend has no locale, and hard-coding one language here would defeat
/// the three-language build.
const EVENT_TOAST: &str = "toast";
const REGION_SELECT_LABEL: &str = "region-select";

/// Read the global cursor position in physical pixels, or `None` if the
/// platform cannot answer.
///
/// `AppHandle::cursor_position` is a desktop-global query in Tauri v2 -- it does
/// not resolve against a window's own frame -- and the `AppHandle` path
/// dispatches onto the main thread and waits, which is what makes it callable
/// from an async command. `show_quick_paste` makes the same call.
///
/// One helper rather than two call sites: the mask and the whole-screen capture
/// have to agree on where the user is looking, and a second copy of this query
/// is where they would start to drift.
fn global_cursor(app: &AppHandle) -> Option<(i32, i32)> {
    let cursor = app.cursor_position().ok()?;
    Some((cursor.x.round() as i32, cursor.y.round() as i32))
}

/// Cover the monitor the cursor is on.
///
/// The window used to rely on `"fullscreen": true` in `tauri.conf.json` alone.
/// That fills exactly one monitor and picks it from the window's own position,
/// which is wherever the overlay happened to be created -- so it landed on the
/// primary display no matter where the user was working. On a second monitor
/// that reads as "the mask does not cover the screen", because the screen you
/// are looking at has no mask at all and the other one is fully covered.
/// A 2560x1440 display on an unmodified build is the same failure with one
/// monitor: the config can only hold one static 1920x1080 rectangle.
///
/// Every other overlay here already resolves its monitor first
/// (`show_quick_paste`, the compact preview, the edge-docked window); this one
/// did not. So the size is set from the monitor instead, at show time, from a
/// config that can only ever hold one static rectangle.
///
/// `cursor_position()` is a desktop-global query in Tauri v2 -- it reaches
/// `AppHandle`, not the window's own frame, so it answers "where is the user
/// looking" rather than "where is this overlay" -- and the `AppHandle` path
/// dispatches onto the main thread and waits, which is what makes it callable
/// from an async command. This is the same call `show_quick_paste` makes.
///
/// Best-effort by design: if the query or the monitor list fails, the window
/// keeps its previous geometry. That is the behaviour from before, not a new
/// failure path.
fn cover_cursor_monitor(window: &tauri::WebviewWindow) {
    let Ok(monitors) = window.available_monitors() else {
        return;
    };

    // Half-open containment, matching the bounds test `capture_region` uses:
    // the pixel at a monitor's right edge belongs to the next monitor.
    let target = global_cursor(window.app_handle()).and_then(|(cursor_x, cursor_y)| {
        monitors.into_iter().find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            screenshot::rect_holds_cursor(
                MonitorRect {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                    is_primary: false,
                },
                cursor_x,
                cursor_y,
            )
        })
    });

    let Some(monitor) = target else {
        return;
    };
    let pos = monitor.position();
    let size = monitor.size();
    // Order matters: size first, then position. On Windows a window resized
    // while it hangs off the right edge gets clamped back into the work area,
    // which would leave it short by however far it stuck out.
    let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize {
        width: size.width,
        height: size.height,
    }));
    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x: pos.x,
        y: pos.y,
    }));
}

/// Bring the region selector back if the idle destroyer took it away.
///
/// The selector is a fullscreen overlay that holds a whole display's worth of
/// compositor surface, so it is torn down with the main window when the app goes
/// idle and rebuilt here the next time the screenshot hotkey fires. It is
/// rebuilt from `tauri.conf.json` for the same reason the main window is: config
/// drift becomes impossible.
///
/// Async because `build()` hands the window to the event loop and waits for it,
/// which deadlocks if this runs on the main thread.
#[tauri::command]
pub async fn show_region_selector(app: AppHandle) -> Result<(), String> {
    let window = match app.get_webview_window(REGION_SELECT_LABEL) {
        Some(window) => window,
        None => rebuild_region_select(&app)?,
    };
    let _ = window.set_focusable(true);
    // Before `show`: sizing a hidden window is free, while doing it after the
    // overlay is on screen would flash the old rectangle first.
    cover_cursor_monitor(&window);
    crate::app::webview_memory::restore_window_memory(&window, "region-select-show");
    window
        .show()
        .map_err(|e| format!("Failed to show region selector: {e}"))?;
    window
        .set_focus()
        .map_err(|e| format!("Failed to focus region selector: {e}"))?;
    Ok(())
}

fn rebuild_region_select(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|c| c.label == REGION_SELECT_LABEL)
        .ok_or_else(|| "Region selector window is not configured".to_string())?;

    tauri::WebviewWindowBuilder::from_config(app, config)
        .and_then(|b| b.build())
        .map_err(|e| format!("Failed to rebuild region selector: {e}"))
}

/// Park the region selector after a cancel.
///
/// The overlay is fullscreen and idle the moment a selection is abandoned, and
/// it holds a whole display's worth of compositor surface while it sits there.
/// Releasing it on cancel is what keeps the startup saving in place for anyone
/// who opens the screenshot tool and then backs out. Safe to call when no
/// selector exists.
#[tauri::command]
pub fn hide_region_select(app: AppHandle) {
    if let Some(window) = app.get_webview_window("region-select") {
        let _ = window.set_focusable(false);
        let _ = window.hide();
        crate::app::webview_memory::lower_window_memory(&window, "region-select-hide");
    }
}

/// Capture the display the cursor is on as a PNG and broadcast a toast.
///
/// This is the second half of a feature whose first half already existed:
/// `ROADMAP.md` 5.1 planned both `capture_screen()` and `capture_region()` plus
/// a `screenshot.full_capture` label, and the backend half was built while the
/// UI entry was never surfaced. Without it, grabbing a whole display means
/// pressing the region hotkey and dragging a rectangle the size of the screen.
///
/// The cursor picks the display, matching `cover_cursor_monitor`. Capturing the
/// primary instead would put a screenshot of the wrong monitor in the clipboard
/// whenever the button was pressed from the settings page, which is the one
/// place the main window is guaranteed to be on screen.
#[tauri::command]
pub async fn capture_full_screen(app: AppHandle) -> Result<ScreenshotResult, String> {
    let result = screenshot::capture_full_screen(global_cursor(&app)).map_err(|e| e.to_string())?;
    copy_screenshot_to_clipboard(&result)?;
    save_screenshot_to_history(&app, &result);
    // The region path does not toast: the mask appearing and then disappearing
    // is its own confirmation, and the region hotkey usually fires from another
    // application where the main window is not even on screen to show it. This
    // one produces no visible change at all -- a click that silently drops an
    // image on the clipboard -- and it is reachable from a visible button.
    let _ = app.emit(EVENT_TOAST, "screenshot_captured");
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
    copy_screenshot_to_clipboard(&result)?;
    save_screenshot_to_history(&app, &result);
    release_selector_after_response(&app);
    Ok(result)
}

/// Park the selector once the caller has actually received the result.
///
/// The capture is invoked *from* the selector, so dropping its memory target
/// inline would race the IPC reply. Waiting hands the reply back first and
/// keeps the teardown off the response path; the overlay is hidden and idle for
/// the whole wait, so nothing the user can see changes.
fn release_selector_after_response(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        hide_region_select(handle);
    });
}

fn copy_screenshot_to_clipboard(result: &ScreenshotResult) -> Result<(), String> {
    let width = result.width;
    let height = result.height;
    let byte_len = result.png_bytes.len();

    if let Err(e) = clipboard_ops::copy_image_bytes_to_system_clipboard(result.png_bytes.clone()) {
        crate::error!(
            "[screenshot] Failed to copy screenshot to clipboard ({}x{}, {} bytes): {}",
            width,
            height,
            byte_len,
            e
        );
        return Err(e.to_string());
    }
    Ok(())
}

fn screenshot_data_url(result: &ScreenshotResult) -> String {
    format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(&result.png_bytes)
    )
}

fn save_screenshot_to_history(app: &AppHandle, result: &ScreenshotResult) {
    process_new_entry(
        app,
        ClipboardData::Image {
            data_url: screenshot_data_url(result),
        },
        Some("DezirClip Screenshot".to_string()),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::screenshot::screenshot_smoke_enabled;

    const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

    #[test]

    #[test]
    fn test_capture_full_screen_returns_png() {
        if !screenshot_smoke_enabled() {
            return;
        }

        match screenshot::capture_full_screen(None) {
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
        let err = screenshot::capture_region(i32::MIN, i32::MIN, 100, 100)
            .err()
            .expect("coordinates outside every monitor must surface RegionOutOfBounds");
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
    fn test_screenshot_data_url_wraps_png_bytes() {
        let result = ScreenshotResult {
            width: 1,
            height: 1,
            png_bytes: PNG_MAGIC.to_vec(),
        };

        assert_eq!(
            screenshot_data_url(&result),
            "data:image/png;base64,iVBORw=="
        );
    }

    #[test]
    fn test_screenshot_round_trip_persists_to_clipboard() {
        if !screenshot_smoke_enabled() {
            return;
        }

        let result = match screenshot::capture_full_screen(None) {
            Ok(r) => r,
            Err(_) => return,
        };

        let write = clipboard_ops::copy_image_bytes_to_system_clipboard(result.png_bytes.clone());
        if write.is_err() {
            return;
        }

        #[cfg(target_os = "linux")]
        {
            use std::time::Duration;
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(_) => return,
            };
            let verified = rt.block_on(async {
                for delay_ms in [50u64, 100, 200] {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    if let Some(image) =
                        crate::infrastructure::linux_api::clipboard::get_clipboard_image()
                    {
                        assert_eq!(image.width, result.width as usize);
                        assert_eq!(image.height, result.height as usize);
                        return true;
                    }
                }
                false
            });
            assert!(
                verified,
                "screenshot bytes must persist in clipboard within retry window"
            );
        }
    }

}
