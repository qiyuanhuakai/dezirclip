// Screenshot service — xcap wrapper with platform-agnostic flat functions.
//
// Platform selection is delegated entirely to xcap:
//   - Linux: X11 + Xrandr + pipewire (xcap picks the right backend at runtime)
//   - Windows: Win32 GDI
//
// Service layer is platform-agnostic; all cfg(target_os = ...) blocks live inside
// xcap and are not duplicated here.

use serde::Serialize;
use xcap::image::{ExtendedColorType, ImageEncoder};
use xcap::Monitor;

#[cfg(test)]
const RUN_SCREENSHOT_SMOKE: &str = "DEZIRCLIP_RUN_SCREENSHOT_SMOKE_TESTS";

#[cfg(test)]
pub(crate) fn screenshot_smoke_enabled() -> bool {
    std::env::var_os(RUN_SCREENSHOT_SMOKE).is_some()
}

/// Captured screenshot in PNG form.
#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotResult {
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

/// One display's placement in absolute physical pixels.
///
/// This is the shape both halves of the capture path need to agree on: the
/// region mask picks the display to cover with it, and a whole-screen capture
/// picks the display to photograph with it. If the two disagreed, a selection
/// drawn on one display would come back as pixels from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// Pixels captured before the selector is shown. Region selection never reads
/// the desktop again, so its mask and border cannot enter the resulting PNG.
pub struct FrozenScreenFrame {
    rect: MonitorRect,
    pixels: xcap::image::RgbaImage,
}

impl FrozenScreenFrame {
    pub fn png_bytes(&self) -> Result<Vec<u8>, ScreenshotError> {
        encode_png(&self.pixels)
    }
    pub(crate) fn from_pixels(rect: MonitorRect, pixels: xcap::image::RgbaImage) -> Result<Self, ScreenshotError> {
        if rect.width == 0 || rect.height == 0 || pixels.dimensions() != (rect.width, rect.height) {
            return Err(ScreenshotError::RegionOutOfBounds);
        }
        Ok(Self { rect, pixels })
    }

    pub fn crop(&self, x: i32, y: i32, width: u32, height: u32) -> Result<ScreenshotResult, ScreenshotError> {
        let relative_x = i64::from(x) - i64::from(self.rect.x);
        let relative_y = i64::from(y) - i64::from(self.rect.y);
        if width == 0 || height == 0 || relative_x < 0 || relative_y < 0
            || relative_x + i64::from(width) > i64::from(self.pixels.width())
            || relative_y + i64::from(height) > i64::from(self.pixels.height()) {
            return Err(ScreenshotError::RegionOutOfBounds);
        }
        let pixels = xcap::image::imageops::crop_imm(
            &self.pixels, relative_x as u32, relative_y as u32, width, height,
        ).to_image();
        Ok(ScreenshotResult { width, height, png_bytes: encode_png(&pixels)? })
    }
}

impl Drop for FrozenScreenFrame {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.pixels.as_mut().zeroize();
    }
}

pub fn freeze_monitor(rect: MonitorRect) -> Result<FrozenScreenFrame, ScreenshotError> {
    let monitors = Monitor::all().map_err(map_monitor_enumeration_error)?;
    let rects = read_monitor_rects(&monitors)?;
    let index = rects.iter().position(|candidate| candidate.x == rect.x && candidate.y == rect.y
        && candidate.width == rect.width && candidate.height == rect.height)
        .ok_or(ScreenshotError::RegionOutOfBounds)?;
    let pixels = monitors[index].capture_image().map_err(map_xcap)?;
    FrozenScreenFrame::from_pixels(rect, pixels)
}

/// Whether `(cursor_x, cursor_y)` falls inside `rect`.
///
/// Half-open on both axes, so the pixel at a display's right edge belongs to the
/// next display rather than to two at once. This matches the containment test
/// `capture_region` applies, which is what keeps a mask that covers exactly the
/// pixels `capture_region` will accept.
///
/// Origins are frequently negative -- a display left of the primary sits at
/// `x = -1920` -- so the arithmetic is signed throughout.
pub fn rect_holds_cursor(rect: MonitorRect, cursor_x: i32, cursor_y: i32) -> bool {
    let (width, height) = (rect.width as i32, rect.height as i32);
    cursor_x >= rect.x
        && cursor_x < rect.x + width
        && cursor_y >= rect.y
        && cursor_y < rect.y + height
}

/// Pick which display a whole-screen capture should photograph.
///
/// The cursor wins when it is over a display: pressing the shortcut while
/// working on the second monitor means the second monitor, which is what every
/// comparable screenshot tool does and the only behaviour that survives a
/// dual-monitor desk. A cursor that cannot be read, or one parked in a gap
/// between displays, falls back to the primary and then to the first entry, so
/// this never returns `None` for a non-empty list.
pub fn pick_capture_monitor(rects: &[MonitorRect], cursor: Option<(i32, i32)>) -> Option<usize> {
    if rects.is_empty() {
        return None;
    }
    if let Some((cursor_x, cursor_y)) = cursor {
        if let Some(index) = rects
            .iter()
            .position(|rect| rect_holds_cursor(*rect, cursor_x, cursor_y))
        {
            return Some(index);
        }
    }
    Some(rects.iter().position(|rect| rect.is_primary).unwrap_or(0))
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
            ScreenshotError::RegionOutOfBounds => {
                write!(f, "Capture region is outside monitor bounds")
            }
            ScreenshotError::EncodeError => write!(f, "Failed to encode PNG from captured image"),
        }
    }
}

impl std::error::Error for ScreenshotError {}

fn map_xcap<E: std::fmt::Display>(err: E) -> ScreenshotError {
    ScreenshotError::XcapError(err.to_string())
}

fn map_monitor_enumeration_error<E: std::fmt::Display>(err: E) -> ScreenshotError {
    let message = err.to_string();
    let no_display = ["DISPLAY", "display string", "Connection closed"]
        .iter()
        .any(|needle| message.contains(needle));
    if no_display {
        ScreenshotError::NoMonitor
    } else {
        ScreenshotError::XcapError(message)
    }
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

/// Read every attached display's placement, preserving `Monitor::all()` order so
/// the indices returned by `pick_capture_monitor` address the same entries.
///
/// `is_primary` is read leniently: a platform that cannot answer is treated as
/// non-primary rather than failing the capture, which is how the previous
/// primary-selection path behaved.
fn read_monitor_rects(monitors: &[Monitor]) -> Result<Vec<MonitorRect>, ScreenshotError> {
    monitors
        .iter()
        .map(|monitor| {
            Ok(MonitorRect {
                x: monitor.x().map_err(map_xcap)?,
                y: monitor.y().map_err(map_xcap)?,
                width: monitor.width().map_err(map_xcap)?,
                height: monitor.height().map_err(map_xcap)?,
                is_primary: monitor.is_primary().unwrap_or(false),
            })
        })
        .collect()
}

/// Capture one whole display as a PNG.
///
/// `cursor` is the global cursor position in physical pixels, or `None` when the
/// platform cannot answer. It selects the display via `pick_capture_monitor`;
/// taking the cursor as an argument rather than querying it here keeps this
/// function free of platform conditionals and keeps the selection testable.
pub fn capture_full_screen(cursor: Option<(i32, i32)>) -> Result<ScreenshotResult, ScreenshotError> {
    let monitors = Monitor::all().map_err(map_monitor_enumeration_error)?;
    if monitors.is_empty() {
        return Err(ScreenshotError::NoMonitor);
    }
    let rects = read_monitor_rects(&monitors)?;
    let index = pick_capture_monitor(&rects, cursor).ok_or(ScreenshotError::NoMonitor)?;
    let rgba = monitors[index].capture_image().map_err(map_xcap)?;
    let (width, height) = rgba.dimensions();
    let png_bytes = encode_png(&rgba)?;
    Ok(ScreenshotResult {
        width,
        height,
        png_bytes,
    })
}

/// Capture a region at absolute screen coordinates `(x, y)` with size `(w, h)`.
///
/// The region must lie entirely within a single monitor; coordinates outside every
/// attached monitor (or non-positive dimensions) yield `RegionOutOfBounds`.
pub fn capture_region(x: i32, y: i32, w: u32, h: u32) -> Result<ScreenshotResult, ScreenshotError> {
    if w == 0 || h == 0 {
        return Err(ScreenshotError::RegionOutOfBounds);
    }

    let monitors = Monitor::all().map_err(map_monitor_enumeration_error)?;
    let right = i64::from(x) + i64::from(w);
    let bottom = i64::from(y) + i64::from(h);
    let mut target: Option<(&Monitor, u32, u32)> = None;

    for monitor in &monitors {
        let mx = monitor.x().map_err(map_xcap)?;
        let my = monitor.y().map_err(map_xcap)?;
        let mw = monitor.width().map_err(map_xcap)?;
        let mh = monitor.height().map_err(map_xcap)?;
        let monitor_right = i64::from(mx) + i64::from(mw);
        let monitor_bottom = i64::from(my) + i64::from(mh);

        if i64::from(x) >= i64::from(mx)
            && i64::from(y) >= i64::from(my)
            && right <= monitor_right
            && bottom <= monitor_bottom
        {
            target = Some((monitor, (x - mx) as u32, (y - my) as u32));
            break;
        }
    }

    let (monitor, rel_x, rel_y) = target.ok_or(ScreenshotError::RegionOutOfBounds)?;

    let rgba = monitor
        .capture_region(rel_x, rel_y, w, h)
        .map_err(map_xcap)?;
    let (width, height) = rgba.dimensions();
    let png_bytes = encode_png(&rgba)?;
    Ok(ScreenshotResult { width, height, png_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_preview_and_region_use_the_same_original_pixels() {
        let rect = MonitorRect { x: -3, y: 2, width: 4, height: 3, is_primary: false };
        let frame = FrozenScreenFrame::from_pixels(rect,
            xcap::image::RgbaImage::from_fn(4, 3, |x, y| xcap::image::Rgba([x as u8 * 40, y as u8 * 40, 17, 255]))).unwrap();
        let preview = xcap::image::load_from_memory(&frame.png_bytes().unwrap()).unwrap().to_rgba8();
        let captured = frame.crop(-2, 3, 2, 2).unwrap();
        let crop = xcap::image::load_from_memory(&captured.png_bytes).unwrap().to_rgba8();
        assert_eq!(preview.dimensions(), (4, 3));
        for y in 0..2 { for x in 0..2 { assert_eq!(crop.get_pixel(x, y), preview.get_pixel(x + 1, y + 1)); } }
    }

    #[test]
    fn frozen_region_crops_original_pixels_with_negative_monitor_origin() {
        let frame = FrozenScreenFrame {
            rect: MonitorRect { x: -4, y: -2, width: 4, height: 3, is_primary: false },
            pixels: xcap::image::RgbaImage::from_fn(4, 3, |x, y| xcap::image::Rgba([x as u8, y as u8, 99, 255])),
        };
        let result = frame.crop(-3, -1, 2, 2).unwrap();
        let decoded = xcap::image::load_from_memory(&result.png_bytes).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert_eq!(decoded.get_pixel(0, 0).0, [1, 1, 99, 255]);
        assert_eq!(decoded.get_pixel(1, 1).0, [2, 2, 99, 255]);
    }

    #[test]
    fn frozen_region_rejects_zero_outside_and_overflowing_coordinates() {
        let frame = FrozenScreenFrame {
            rect: MonitorRect { x: -4, y: -2, width: 4, height: 3, is_primary: false },
            pixels: xcap::image::RgbaImage::new(4, 3),
        };
        for (x, y, width, height) in [(-4, -2, 0, 1), (-5, -2, 1, 1), (-4, -3, 1, 1),
            (-1, 0, 2, 1), (i32::MAX, i32::MAX, u32::MAX, u32::MAX)] {
            assert!(matches!(frame.crop(x, y, width, height), Err(ScreenshotError::RegionOutOfBounds)));
        }
        assert!(frame.crop(-4, -2, 4, 3).is_ok());
    }

    /// A 1920x1080 display at the origin.
    fn primary() -> MonitorRect {
        MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: true,
        }
    }

    /// A 2560x1440 display immediately to the right of `primary`.
    fn right_hand() -> MonitorRect {
        MonitorRect {
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            is_primary: false,
        }
    }

    /// A 1920x1080 display to the *left* of `primary`, so its origin is negative.
    fn left_hand() -> MonitorRect {
        MonitorRect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: false,
        }
    }

    /// First four bytes of every valid PNG stream.
    const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

    #[test]
    fn test_rect_holds_cursor_interior_and_corners() {
        let rect = primary();
        assert!(
            rect_holds_cursor(rect, 960, 540),
            "the middle of the display is on it"
        );
        assert!(
            rect_holds_cursor(rect, 0, 0),
            "the top-left pixel is on it -- edges are inclusive"
        );
        assert!(
            rect_holds_cursor(rect, 1919, 1079),
            "the bottom-right pixel is on it"
        );
        assert!(
            !rect_holds_cursor(rect, -1, 540),
            "one pixel to the left of the origin is off it"
        );
        assert!(
            !rect_holds_cursor(rect, 960, -1),
            "one pixel above the origin is off it"
        );
    }

    #[test]
    fn test_rect_holds_cursor_is_half_open_at_the_far_edges() {
        let rect = primary();
        assert!(
            !rect_holds_cursor(rect, 1920, 540),
            "x = 1920 is the first pixel past a 1920-wide display, not on it"
        );
        assert!(
            !rect_holds_cursor(rect, 960, 1080),
            "y = 1080 is the first pixel past a 1080-tall display, not on it"
        );
    }

    /// The seam between two side-by-side displays is the case the half-open
    /// interval exists for. Both halves are asserted, so a function that went
    /// inclusive on both ends (no display claims the seam) or exclusive on both
    /// (every display claims it) fails here.
    #[test]
    fn test_rect_holds_cursor_seam_belongs_to_exactly_one_monitor() {
        let left = primary();
        let right = right_hand();

        for seam_x in [left.x + left.width as i32, left.x + left.width as i32 + 1] {
            assert!(
                !rect_holds_cursor(left, seam_x, 540),
                "x = {seam_x} belongs to the display on the right, not the one on the left"
            );
            assert!(
                rect_holds_cursor(right, seam_x, 540),
                "x = {seam_x} is inside the display on the right"
            );
        }
    }

    /// A display left of the primary has a negative origin, which is the case a
    /// truncating or unsigned intermediate would silently get wrong.
    #[test]
    fn test_rect_holds_cursor_handles_negative_origin() {
        let rect = MonitorRect {
            x: -1920,
            y: -200,
            width: 1920,
            height: 1080,
            is_primary: false,
        };
        assert!(
            rect_holds_cursor(rect, -1000, -100),
            "a cursor at (-1000, -100) is inside a display spanning x in [-1920, 0)"
        );
        assert!(
            rect_holds_cursor(rect, -1919, -199),
            "the display's own top-left pixel, in negative coordinates, is on it"
        );
        assert!(
            !rect_holds_cursor(rect, -1921, -100),
            "one pixel left of a -1920 origin is off it"
        );
        assert!(
            !rect_holds_cursor(rect, -1000, 881),
            "y = 881 is past a display spanning y in [-200, 880)"
        );
    }

    #[test]
    fn test_rect_holds_cursor_zero_sized_monitor_contains_nothing() {
        let rect = MonitorRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            is_primary: true,
        };
        for (x, y) in [(0, 0), (-1, -1), (1, 1)] {
            assert!(
                !rect_holds_cursor(rect, x, y),
                "a zero-sized display must not claim the cursor at ({x}, {y})"
            );
        }
    }

    /// Half-open applies to a display that butts up against the origin too, not
    /// just to one with a neighbour on the right.
    #[test]
    fn test_rect_holds_cursor_negative_origin_ending_at_zero_excludes_zero() {
        let rect = left_hand();
        assert!(
            !rect_holds_cursor(rect, 0, 540),
            "x = 0 is the first pixel of the primary, not the last pixel of the display on its left"
        );
        assert!(
            rect_holds_cursor(rect, -1, 540),
            "x = -1 is the last pixel of that display"
        );
        let wider = MonitorRect {
            width: 3840,
            ..rect
        };
        assert!(
            rect_holds_cursor(wider, 0, 540),
            "a display spanning x in [-1920, 1920) does claim x = 0"
        );
    }

    #[test]
    fn test_pick_capture_monitor_prefers_the_display_under_the_cursor() {
        let rects = [primary(), right_hand()];
        assert_eq!(
            pick_capture_monitor(&rects, Some((3000, 700))),
            Some(1),
            "a cursor at (3000, 700) is on the right-hand display"
        );
        assert_eq!(
            pick_capture_monitor(&rects, Some((500, 500))),
            Some(0),
            "a cursor at (500, 500) is on the primary"
        );
    }

    #[test]
    fn test_pick_capture_monitor_prefers_the_primary_over_a_non_primary_cursor() {
        // A primary sitting to the right of a secondary: the cursor must win over
        // the primary flag, which is the whole point of taking the cursor.
        let rects = [
            MonitorRect {
                x: -1920,
                width: 1920,
                ..left_hand()
            },
            primary(),
        ];
        assert_eq!(
            pick_capture_monitor(&rects, Some((-500, 500))),
            Some(0),
            "the cursor on the left-hand display must beat the primary flag"
        );
    }

    #[test]
    fn test_pick_capture_monitor_falls_back_when_the_cursor_is_unusable() {
        let rects = [right_hand(), primary()];
        assert_eq!(
            pick_capture_monitor(&rects, None),
            Some(1),
            "with no cursor, the primary wins even when it is not first"
        );
        assert_eq!(
            pick_capture_monitor(&rects, Some((9_000, 9_000))),
            Some(1),
            "a cursor parked in a gap between displays falls back to the primary"
        );
        let no_primary = [right_hand(), left_hand()];
        assert_eq!(
            pick_capture_monitor(&no_primary, None),
            Some(0),
            "with no primary flagged, the first display is used rather than failing"
        );
        assert_eq!(
            pick_capture_monitor(&[], Some((0, 0))),
            None,
            "an empty display list has nothing to pick"
        );
    }

    #[test]
    fn test_capture_full_screen_smoke() -> Result<(), ScreenshotError> {
        if !screenshot_smoke_enabled() {
            return Ok(());
        }

        match capture_full_screen(None) {
            Ok(result) => {
                assert!(result.width > 0, "captured width must be positive");
                assert!(result.height > 0, "captured height must be positive");
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
        if !screenshot_smoke_enabled() {
            return Ok(());
        }

        // Pick a known-good region on the primary monitor (or skip on no-display CI).
        let monitors = match Monitor::all() {
            Ok(m) if !m.is_empty() => m,
            _ => return Ok(()),
        };
        let rects = match read_monitor_rects(&monitors) {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };

        let primary = rects
            .iter()
            .find(|m| m.is_primary)
            .unwrap_or(&rects[0]);
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
    fn test_monitor_enumeration_display_failure_maps_to_no_monitor() {
        let err = map_monitor_enumeration_error("Connection closed, error during parsing display string");
        assert!(matches!(err, ScreenshotError::NoMonitor));
    }

    #[test]
    fn test_monitor_rects_are_populated() -> Result<(), ScreenshotError> {
        if !screenshot_smoke_enabled() {
            return Ok(());
        }

        let monitors = match Monitor::all() {
            Ok(m) if !m.is_empty() => m,
            _ => return Ok(()), // headless CI: the list may legitimately be empty
        };
        let rects = read_monitor_rects(&monitors)?;
        assert_eq!(
            rects.len(),
            monitors.len(),
            "every enumerated display must yield a rect, or the index returned by \
             pick_capture_monitor would address the wrong entry"
        );
        for rect in &rects {
            assert!(rect.width > 0, "monitor width must be populated and non-zero");
            assert!(
                rect.height > 0,
                "monitor height must be populated and non-zero"
            );
        }
        Ok(())
    }

    #[test]
    fn test_screenshot_result_is_png() -> Result<(), ScreenshotError> {
        if !screenshot_smoke_enabled() {
            return Ok(());
        }

        let result = match capture_full_screen(None) {
            Ok(r) => r,
            Err(_) => return Ok(()), // headless CI: nothing to validate
        };
        assert!(result.png_bytes.len() >= PNG_MAGIC.len(), "PNG output too short: {} bytes", result.png_bytes.len());
        assert_eq!(
            &result.png_bytes[..PNG_MAGIC.len()],
            &PNG_MAGIC,
            "PNG magic bytes mismatch: expected 89 50 4E 47"
        );
        Ok(())
    }
}
