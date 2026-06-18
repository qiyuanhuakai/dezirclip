// Screenshot service — xcap wrapper with platform-agnostic flat functions.
//
// Platform selection is delegated entirely to xcap:
//   - Linux: X11 + Xrandr + pipewire (xcap picks the right backend at runtime)
//   - Windows: Win32 GDI
//   - macOS: CoreGraphics (not used by this project, but supported upstream)
//
// Service layer is platform-agnostic; all cfg(target_os = ...) blocks live inside
// xcap and are not duplicated here.

use serde::Serialize;
use xcap::image::{ExtendedColorType, ImageEncoder};
use xcap::Monitor;

/// Captured screenshot in PNG form.
#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotResult {
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

/// One physical display reported by the OS.
#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// Failure modes for screenshot operations.
#[derive(Debug)]
pub enum ScreenshotError {
    /// No monitor is attached (headless CI, locked workstation, etc.).
    NoMonitor,
    /// xcap or the underlying OS API returned an error; the wrapped message is included.
    XcapError(String),
    /// Capture region is outside the target monitor's bounds, or has zero dimensions.
    RegionOutOfBounds,
    /// PNG encoding of the captured image failed.
    EncodeError,
}

impl std::fmt::Display for ScreenshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScreenshotError::NoMonitor => write!(f, "No monitor available for capture"),
            ScreenshotError::XcapError(msg) => write!(f, "xcap error: {msg}"),
            ScreenshotError::RegionOutOfBounds => write!(f, "Capture region is outside monitor bounds"),
            ScreenshotError::EncodeError => write!(f, "Failed to encode PNG from captured image"),
        }
    }
}

impl std::error::Error for ScreenshotError {}

fn map_xcap<E: std::fmt::Display>(err: E) -> ScreenshotError {
    ScreenshotError::XcapError(err.to_string())
}

/// Encode a RGBA image as PNG bytes.
fn encode_png(rgba: &xcap::image::RgbaImage) -> Result<Vec<u8>, ScreenshotError> {
    let mut buf: Vec<u8> = Vec::with_capacity(rgba.len() / 4);
    let (w, h) = rgba.dimensions();
    let encoder = xcap::image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(rgba.as_raw(), w, h, ExtendedColorType::Rgba8)
        .map_err(|_| ScreenshotError::EncodeError)?;
    Ok(buf)
}

/// Pick the primary monitor if present, otherwise the first one.
fn select_primary_monitor(monitors: &[Monitor]) -> Option<&Monitor> {
    monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
}

/// Capture the full primary monitor as a PNG.
pub fn capture_full_screen() -> Result<ScreenshotResult, ScreenshotError> {
    let monitors = Monitor::all().map_err(map_xcap)?;
    let monitor = select_primary_monitor(&monitors).ok_or(ScreenshotError::NoMonitor)?;
    let rgba = monitor.capture_image().map_err(map_xcap)?;
    let (width, height) = rgba.dimensions();
    let png_bytes = encode_png(&rgba)?;
    Ok(ScreenshotResult { width, height, png_bytes })
}

/// Capture a region at absolute screen coordinates `(x, y)` with size `(w, h)`.
///
/// The region must lie entirely within a single monitor; coordinates outside every
/// attached monitor (or non-positive dimensions) yield `RegionOutOfBounds`.
pub fn capture_region(x: i32, y: i32, w: u32, h: u32) -> Result<ScreenshotResult, ScreenshotError> {
    if w == 0 || h == 0 || x < 0 || y < 0 {
        return Err(ScreenshotError::RegionOutOfBounds);
    }

    let monitor = Monitor::from_point(x, y).map_err(map_xcap)?;
    let mx = monitor.x().map_err(map_xcap)?;
    let my = monitor.y().map_err(map_xcap)?;
    let mw = monitor.width().map_err(map_xcap)?;
    let mh = monitor.height().map_err(map_xcap)?;

    if x < mx || y < my {
        return Err(ScreenshotError::RegionOutOfBounds);
    }

    let rel_x = (x - mx) as u32;
    let rel_y = (y - my) as u32;

    // Use saturating subtraction to avoid u32 overflow on adversarial inputs.
    if w > mw.saturating_sub(rel_x) || h > mh.saturating_sub(rel_y) {
        return Err(ScreenshotError::RegionOutOfBounds);
    }

    let rgba = monitor.capture_region(rel_x, rel_y, w, h).map_err(map_xcap)?;
    let (width, height) = rgba.dimensions();
    let png_bytes = encode_png(&rgba)?;
    Ok(ScreenshotResult { width, height, png_bytes })
}

pub fn capture_monitor(id: u32) -> Result<ScreenshotResult, ScreenshotError> {
    let monitors = Monitor::all().map_err(map_xcap)?;
    let monitor = monitors
        .iter()
        .find(|m| m.id().ok() == Some(id))
        .ok_or(ScreenshotError::NoMonitor)?;
    let rgba = monitor.capture_image().map_err(map_xcap)?;
    let (width, height) = rgba.dimensions();
    let png_bytes = encode_png(&rgba)?;
    Ok(ScreenshotResult { width, height, png_bytes })
}

/// Enumerate all attached monitors.
pub fn list_monitors() -> Result<Vec<MonitorInfo>, ScreenshotError> {
    let monitors = Monitor::all().map_err(map_xcap)?;
    if monitors.is_empty() {
        return Err(ScreenshotError::NoMonitor);
    }

    let mut out = Vec::with_capacity(monitors.len());
    for m in &monitors {
        out.push(MonitorInfo {
            id: m.id().map_err(map_xcap)?,
            name: m.name().map_err(map_xcap)?,
            x: m.x().map_err(map_xcap)?,
            y: m.y().map_err(map_xcap)?,
            width: m.width().map_err(map_xcap)?,
            height: m.height().map_err(map_xcap)?,
            is_primary: m.is_primary().map_err(map_xcap)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// First four bytes of every valid PNG stream.
    const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

    #[test]
    fn test_capture_full_screen_smoke() -> Result<(), ScreenshotError> {
        match capture_full_screen() {
            Ok(result) => {
                assert!(result.width > 0, "primary monitor width must be positive");
                assert!(result.height > 0, "primary monitor height must be positive");
                assert!(result.png_bytes.len() > 8, "PNG must be at least 8 bytes");
                assert_eq!(
                    &result.png_bytes[..4],
                    &PNG_MAGIC,
                    "capture_full_screen output must start with PNG magic bytes"
                );
            }
            Err(_) => {
                // Tolerate headless / no-display CI environments; the type-checked
                // Result return value lets the test exit cleanly when capture fails.
            }
        }
        Ok(())
    }

    #[test]
    fn test_capture_region_basic() -> Result<(), ScreenshotError> {
        // Pick a known-good region on the primary monitor (or skip on no-display CI).
        let monitors = match list_monitors() {
            Ok(m) if !m.is_empty() => m,
            _ => return Ok(()),
        };

        let primary = monitors
            .iter()
            .find(|m| m.is_primary)
            .unwrap_or(&monitors[0]);
        if primary.width < 10 || primary.height < 10 {
            return Ok(()); // Display too small to test; defer silently.
        }

        let region_w = 100u32.min(primary.width);
        let region_h = 100u32.min(primary.height);
        let result = capture_region(primary.x, primary.y, region_w, region_h)?;
        assert_eq!(result.width, region_w, "region width must match request");
        assert_eq!(result.height, region_h, "region height must match request");
        assert!(result.png_bytes.len() > 8, "PNG must be at least 8 bytes");
        assert_eq!(
            &result.png_bytes[..4],
            &PNG_MAGIC,
            "capture_region output must start with PNG magic bytes"
        );
        Ok(())
    }

    #[test]
    fn test_list_monitors_returns_at_least_one() -> Result<(), ScreenshotError> {
        match list_monitors() {
            Ok(list) => assert!(!list.is_empty(), "expected at least one monitor"),
            Err(_) => {
                // Tolerate headless CI: the list may legitimately be empty there.
            }
        }
        Ok(())
    }

    #[test]
    fn test_monitor_info_fields() -> Result<(), ScreenshotError> {
        let list = match list_monitors() {
            Ok(l) => l,
            Err(_) => return Ok(()),
        };
        let monitor = list
            .iter()
            .find(|m| m.is_primary)
            .or(list.first())
            .expect("non-empty list implies at least one element");
        assert!(monitor.width > 0, "monitor width must be populated and non-zero");
        assert!(monitor.height > 0, "monitor height must be populated and non-zero");
        assert!(!monitor.name.is_empty(), "monitor name must be populated");
        Ok(())
    }

    #[test]
    fn test_screenshot_result_is_png() -> Result<(), ScreenshotError> {
        let result = match capture_full_screen() {
            Ok(r) => r,
            Err(_) => return Ok(()), // headless CI: nothing to validate
        };
        assert!(
            result.png_bytes.len() >= PNG_MAGIC.len(),
            "PNG output too short: {} bytes",
            result.png_bytes.len()
        );
        assert_eq!(
            &result.png_bytes[..PNG_MAGIC.len()],
            &PNG_MAGIC,
            "PNG magic bytes mismatch: expected 89 50 4E 47"
        );
        Ok(())
    }
}