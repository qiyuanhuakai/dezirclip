use base64::{engine::general_purpose, Engine as _};
use tauri::{AppHandle, Emitter, Manager};

use crate::services::clipboard_ops;
use crate::services::clipboard::{process_new_entry, ClipboardData};
use crate::services::screenshot::{self, MonitorInfo, ScreenshotResult};

const EVENT_SCREENSHOT_COMPLETE: &str = "screenshot:complete";
const REGION_SELECT_LABEL: &str = "region-select";

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
    let target = window
        .cursor_position()
        .ok()
        .and_then(|cursor| {
            let (cursor_x, cursor_y) = (cursor.x.round() as i32, cursor.y.round() as i32);
            monitors
                .into_iter()
                .find(|monitor| monitor_holds_cursor(monitor, cursor_x, cursor_y))
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

/// Whether a cursor at `(cursor_x, cursor_y)` sits on `monitor`.
fn monitor_holds_cursor(monitor: &tauri::Monitor, cursor_x: i32, cursor_y: i32) -> bool {
    let pos = monitor.position();
    let size = monitor.size();
    rect_holds_cursor(pos.x, pos.y, size.width, size.height, cursor_x, cursor_y)
}

/// Whether `(cursor_x, cursor_y)` falls inside the rectangle at `(left, top)`
/// sized `(width, height)` in physical pixels.
///
/// Split out from `monitor_holds_cursor` because `tauri::Monitor` has no public
/// constructor: a test cannot build one, so testing the wrapper would assert
/// nothing. Every edge case that matters -- the half-open seam, a display left
/// of the primary, a degenerate zero-size monitor -- lives in the arithmetic
/// below and is reachable from here.
///
/// Half-open on both axes, so the pixel at a monitor's right edge belongs to the
/// next monitor rather than to two at once, matching the bounds test
/// `capture_region` applies. Origins are frequently negative -- a display left
/// of the primary sits at `x = -1920` -- so the arithmetic is signed throughout.
fn rect_holds_cursor(
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    cursor_x: i32,
    cursor_y: i32,
) -> bool {
    let (width, height) = (width as i32, height as i32);
    cursor_x >= left && cursor_x < left + width && cursor_y >= top && cursor_y < top + height
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

/// Capture the primary monitor as a PNG and broadcast a `screenshot:complete`
/// event with the same payload the command returns to the caller.
#[tauri::command]
pub async fn capture_full_screen(app: AppHandle) -> Result<ScreenshotResult, String> {
    let result = screenshot::capture_full_screen().map_err(|e| e.to_string())?;
    copy_screenshot_to_clipboard(&result)?;
    save_screenshot_to_history(&app, &result);
    emit_screenshot_complete(&app, &result);
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
    emit_screenshot_complete(&app, &result);
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

/// Enumerate every attached monitor.
#[tauri::command]
pub async fn list_monitors(_app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    screenshot::list_monitors().map_err(|e| e.to_string())
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

fn emit_screenshot_complete(app: &AppHandle, result: &ScreenshotResult) {
    let _ = app.emit(EVENT_SCREENSHOT_COMPLETE, result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::screenshot::screenshot_smoke_enabled;

    const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

    #[test]
    fn test_rect_holds_cursor_interior_and_corners() {
        let left = 0;
        let top = 0;
        let width = 1920;
        let height = 1080;

        assert!(
            rect_holds_cursor(left, top, width, height, 960, 540),
            "the middle of the display is on it"
        );
        assert!(
            rect_holds_cursor(left, top, width, height, 0, 0),
            "the top-left pixel is on it -- edges are inclusive"
        );
        assert!(
            rect_holds_cursor(left, top, width, height, 1919, 1079),
            "the bottom-right pixel is on it"
        );
        assert!(
            !rect_holds_cursor(left, top, width, height, -1, 540),
            "one pixel to the left of the origin is off it"
        );
        assert!(
            !rect_holds_cursor(left, top, width, height, 960, -1),
            "one pixel above the origin is off it"
        );
    }

    #[test]
    fn test_rect_holds_cursor_is_half_open_at_the_far_edges() {
        // A 1920-wide display starting at the origin occupies x in [0, 1920).
        assert!(
            !rect_holds_cursor(0, 0, 1920, 1080, 1920, 540),
            "x = 1920 is the first pixel past a 1920-wide display, not on it"
        );
        assert!(
            !rect_holds_cursor(0, 0, 1920, 1080, 960, 1080),
            "y = 1080 is the first pixel past a 1080-tall display, not on it"
        );
    }

    /// The seam between two side-by-side displays is the case the half-open
    /// interval exists for. Both halves are asserted, so a function that went
    /// inclusive on both ends (no display claims the seam) or exclusive on both
    /// (every display claims it) fails here.
    #[test]
    fn test_rect_holds_cursor_seam_belongs_to_exactly_one_monitor() {
        let primary_x = 0i32;
        let primary_w = 1920u32;
        let secondary_x = 1920i32;
        let secondary_w = 2560u32;
        // The seam sits one pixel past the left display's width, in absolute
        // coordinates rather than in that display's local frame.
        let seam = primary_x + primary_w as i32;

        for seam_x in [seam, seam + 1] {
            assert!(
                !rect_holds_cursor(primary_x, 0, primary_w, 1080, seam_x, 540),
                "x = {seam_x} belongs to the display on the right, not the one on the left"
            );
            assert!(
                rect_holds_cursor(secondary_x, 0, secondary_w, 1440, seam_x, 540),
                "x = {seam_x} is inside the display on the right"
            );
        }
    }

    /// A display left of the primary has a negative origin, which is the case a
    /// truncating or unsigned intermediate would silently get wrong.
    #[test]
    fn test_rect_holds_cursor_handles_negative_origin() {
        let left = -1920;
        let top = -200;
        let width = 1920;
        let height = 1080;

        assert!(
            rect_holds_cursor(left, top, width, height, -1000, -100),
            "a cursor at (-1000, -100) is inside a display spanning x in [-1920, 0)"
        );
        assert!(
            rect_holds_cursor(left, top, width, height, -1919, -199),
            "the display's own top-left pixel, in negative coordinates, is on it"
        );
        assert!(
            !rect_holds_cursor(left, top, width, height, -1921, -100),
            "one pixel left of a -1920 origin is off it"
        );
        assert!(
            !rect_holds_cursor(left, top, width, height, -1000, 881),
            "y = 881 is past a display spanning y in [-200, 880)"
        );
    }

    #[test]
    fn test_rect_holds_cursor_zero_sized_monitor_contains_nothing() {
        for (x, y) in [(0, 0), (-1, -1), (1, 1)] {
            assert!(
                !rect_holds_cursor(0, 0, 0, 0, x, y),
                "a zero-sized display must not claim the cursor at ({x}, {y})"
            );
        }
    }

    /// Half-open applies to a display that butts up against the origin too, not
    /// just to one with a neighbour on the right.
    #[test]
    fn test_rect_holds_cursor_negative_origin_ending_at_zero_excludes_zero() {
        // A 2560-wide display left of the primary occupies x in [-2560, 0).
        assert!(
            !rect_holds_cursor(-2560, 0, 2560, 1440, 0, 540),
            "x = 0 is the first pixel of the primary, not the last pixel of the display on its left"
        );
        assert!(
            rect_holds_cursor(-2560, 0, 2560, 1440, -1, 540),
            "x = -1 is the last pixel of that display"
        );
        // Widening it so it genuinely spans the origin brings the pixel back,
        // which is what shows the assertion above is about the interval and not
        // about negative arithmetic being broken.
        assert!(
            rect_holds_cursor(-2560, 0, 3840, 1440, 0, 540),
            "a display spanning x in [-2560, 1280) does claim x = 0"
        );
    }

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

        let result = match screenshot::capture_full_screen() {
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
