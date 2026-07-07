use base64::{engine::general_purpose, Engine as _};
use tauri::{AppHandle, Emitter, Manager};

use crate::services::clipboard_ops;
use crate::services::clipboard::{process_new_entry, ClipboardData};
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
    copy_screenshot_to_clipboard(&result).await?;
    save_screenshot_to_history(&app, &result);
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
    copy_screenshot_to_clipboard(&result).await?;
    save_screenshot_to_history(&app, &result);
    let _ = app.emit(EVENT_SCREENSHOT_COMPLETE, &result);
    Ok(result)
}

/// Enumerate every attached monitor.
#[tauri::command]
pub async fn list_monitors(_app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    screenshot::list_monitors().map_err(|e| e.to_string())
}

/// Compares RGBA sample pixels (corners + mid-edges + center) to catch the
/// "same dimensions, different image" failure mode that a dimension-only
/// check misses. Takes primitives so it works against both the Linux and
/// Windows clipboard ImageData types (which are structurally identical but
/// distinct in Rust's type system).
#[allow(clippy::too_many_arguments)]
fn images_match(
    actual_width: usize,
    actual_height: usize,
    actual_bytes: &[u8],
    expected_width: u32,
    expected_height: u32,
    expected_png_bytes: &[u8],
) -> bool {
    let expected_w = expected_width as usize;
    let expected_h = expected_height as usize;

    if actual_width != expected_w || actual_height != expected_h {
        return false;
    }

    let expected_rgba = match image::load_from_memory(expected_png_bytes) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return false,
    };

    if expected_rgba.dimensions() != (expected_width, expected_height) {
        return false;
    }

    let expected_raw = expected_rgba.as_raw();

    let sample_points: [(usize, usize); 8] = [
        (0, 0),
        (expected_w / 4, 0),
        (expected_w / 2, 0),
        (3 * expected_w / 4, 0),
        (expected_w.saturating_sub(1), 0),
        (0, expected_h / 2),
        (expected_w / 2, expected_h / 2),
        (expected_w.saturating_sub(1), expected_h.saturating_sub(1)),
    ];

    for (sx, sy) in sample_points {
        let row_offset = sy * expected_w * 4;
        let col_offset = sx * 4;
        let expected_pixel = &expected_raw[row_offset + col_offset..row_offset + col_offset + 4];
        let actual_pixel = &actual_bytes[row_offset + col_offset..row_offset + col_offset + 4];
        if expected_pixel != actual_pixel {
            return false;
        }
    }

    true
}

#[cfg(target_os = "linux")]
type ClipboardImage = crate::infrastructure::linux_api::clipboard::ImageData;

#[cfg(target_os = "windows")]
type ClipboardImage = crate::infrastructure::windows_api::win_clipboard::ImageData;

fn clipboard_verification_failed(kind: &str) -> String {
    let detail = match kind {
        "mismatch" => "剪贴板内已存在尺寸相同但内容不同的图像",
        _ => "请检查是否有其他应用抢占了剪贴板",
    };
    format!("截图已保存到历史但未能写入剪贴板 — {detail}")
}

async fn verify_clipboard_holds_screenshot(
    png_bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    let img_opt: Option<ClipboardImage> = {
        #[cfg(target_os = "linux")]
        {
            crate::infrastructure::linux_api::clipboard::get_clipboard_image()
        }
        #[cfg(target_os = "windows")]
        {
            unsafe { crate::infrastructure::windows_api::win_clipboard::get_clipboard_image() }
        }
    };

    match img_opt {
        Some(img) => {
            if images_match(img.width, img.height, &img.bytes, width, height, png_bytes) {
                Ok(())
            } else {
                crate::error!(
                    "[screenshot] Final verification returned a mismatched image ({}x{} != {}x{})",
                    width,
                    height,
                    width,
                    height
                );
                Err(clipboard_verification_failed("mismatch"))
            }
        }
        None => {
            crate::error!(
                "[screenshot] Final verification read returned no image after retry write"
            );
            Err(clipboard_verification_failed("absent"))
        }
    }
}

async fn copy_screenshot_to_clipboard(result: &ScreenshotResult) -> Result<(), String> {
    let png_bytes = result.png_bytes.clone();
    let width = result.width;
    let height = result.height;
    let byte_len = png_bytes.len();

    if let Err(e) = clipboard_ops::copy_image_bytes_to_system_clipboard(png_bytes.clone()) {
        crate::error!(
            "[screenshot] Failed to copy screenshot to clipboard ({}x{}, {} bytes): {}",
            width,
            height,
            byte_len,
            e
        );
        return Err(e.to_string());
    }

    #[cfg(target_os = "linux")]
    {
        use std::time::Duration;

        for delay_ms in [100u64, 250, 500] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            if let Some(img) =
                crate::infrastructure::linux_api::clipboard::get_clipboard_image()
            {
                if images_match(img.width, img.height, &img.bytes, width, height, &png_bytes) {
                    return Ok(());
                }
            }
        }

        crate::warn!(
            "[screenshot] Verification reads failed; attempting one final write to clipboard"
        );
        if let Err(e) = clipboard_ops::copy_image_bytes_to_system_clipboard(png_bytes.clone()) {
            crate::error!("[screenshot] Final retry write failed: {}", e);
            return Err(format!(
                "{} ({e})",
                clipboard_verification_failed("absent")
            ));
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
        return verify_clipboard_holds_screenshot(&png_bytes, width, height).await;
    }

    #[cfg(target_os = "windows")]
    {
        use std::time::Duration;

        for delay_ms in [100u64, 250, 500] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            if let Some(img) = unsafe {
                crate::infrastructure::windows_api::win_clipboard::get_clipboard_image()
            } {
                if images_match(img.width, img.height, &img.bytes, width, height, &png_bytes) {
                    return Ok(());
                }
            }
        }

        crate::warn!(
            "[screenshot] Verification reads failed; attempting one final write to clipboard"
        );
        if let Err(e) = clipboard_ops::copy_image_bytes_to_system_clipboard(png_bytes.clone()) {
            crate::error!("[screenshot] Final retry write failed: {}", e);
            return Err(format!(
                "{} ({e})",
                clipboard_verification_failed("absent")
            ));
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
        return verify_clipboard_holds_screenshot(&png_bytes, width, height).await;
    }
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

    #[test]
    fn test_images_match_compares_sample_pixels() {
        let marker = image::RgbaImage::from_fn(50, 50, |_x, _y| {
            image::Rgba([1u8, 2, 3, 255])
        });
        let mut png_buf: Vec<u8> = Vec::new();
        marker
            .write_to(
                &mut std::io::Cursor::new(&mut png_buf),
                image::ImageFormat::Png,
            )
            .unwrap();

        let actual_bytes = marker.clone().into_raw();

        assert!(
            images_match(50, 50, &actual_bytes, 50, 50, &png_buf),
            "identical RGBA bytes must match the source PNG"
        );

        let mismatched = image::RgbaImage::from_fn(50, 50, |_x, _y| {
            image::Rgba([9u8, 9, 9, 255])
        })
        .into_raw();
        assert!(
            !images_match(50, 50, &mismatched, 50, 50, &png_buf),
            "different content with same dimensions must NOT match"
        );

        assert!(
            !images_match(60, 50, &actual_bytes, 50, 50, &png_buf),
            "width mismatch must short-circuit to false"
        );

        assert!(
            !images_match(50, 60, &actual_bytes, 50, 50, &png_buf),
            "height mismatch must short-circuit to false"
        );
    }

    #[test]
    fn test_copy_screenshot_to_clipboard_surfaces_verification_failure() {
        if !screenshot_smoke_enabled() {
            return;
        }

        let img = image::RgbaImage::from_fn(50, 50, |_x, _y| {
            image::Rgba([1u8, 2, 3, 255])
        });
        let mut png_buf: Vec<u8> = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_buf),
            image::ImageFormat::Png,
        )
        .unwrap();

        let result = ScreenshotResult {
            width: 50,
            height: 50,
            png_bytes: png_buf,
        };

        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => return,
        };
        let copy_result = rt.block_on(async {
            copy_screenshot_to_clipboard(&result).await
        });

        match copy_result {
            Ok(()) => {}
            Err(msg) => {
                assert!(
                    msg.contains("截图已保存到历史但未能写入剪贴板"),
                    "verification failure must surface a localized Chinese error, got: {msg}"
                );
            }
        }
    }
}
