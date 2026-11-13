use super::{map_xcap, MonitorRect, ScreenshotError};
use x11rb::{connection::Connection, image::{Image, PixelLayout}, protocol::xproto::ConnectionExt};
use zeroize::Zeroize;

pub(super) fn capture(rect: MonitorRect) -> Result<xcap::image::RgbaImage, ScreenshotError> {
    if rect.width == 0 || rect.height == 0 || u64::from(rect.width) * u64::from(rect.height) > 40_000_000 {
        return Err(ScreenshotError::RegionOutOfBounds);
    }
    crate::runtime_display::validate_x11_display().map_err(map_xcap)?;
    let (connection, screen_index) = x11rb::connect(None).map_err(map_xcap)?;
    let screen = connection.setup().roots.get(screen_index).ok_or(ScreenshotError::NoMonitor)?;
    let geometry = connection.get_geometry(screen.root).map_err(map_xcap)?.reply().map_err(map_xcap)?;
    if rect.x < 0 || rect.y < 0 || i64::from(rect.x) + i64::from(rect.width) > i64::from(geometry.width)
        || i64::from(rect.y) + i64::from(rect.height) > i64::from(geometry.height) {
        return Err(ScreenshotError::RegionOutOfBounds);
    }
    let visual = screen.allowed_depths.iter().flat_map(|depth| &depth.visuals)
        .find(|visual| visual.visual_id == screen.root_visual).ok_or_else(|| map_xcap("X11 root visual unavailable"))?;
    let layout = PixelLayout::from_visual_type(*visual).map_err(map_xcap)?;
    let x = i16::try_from(rect.x).map_err(|_| ScreenshotError::RegionOutOfBounds)?;
    let y = i16::try_from(rect.y).map_err(|_| ScreenshotError::RegionOutOfBounds)?;
    let width = u16::try_from(rect.width).map_err(|_| ScreenshotError::RegionOutOfBounds)?;
    let height = u16::try_from(rect.height).map_err(|_| ScreenshotError::RegionOutOfBounds)?;
    // Image handles scanline padding and image byte order independently of the
    // bitmap bit order. A 30-bit root stores RGB10 channels in a 32-bit pixel.
    let (mut source, _) = Image::get(&connection, screen.root, x, y, width, height).map_err(map_xcap)?;
    let pixels = xcap::image::RgbaImage::from_fn(rect.width, rect.height, |x, y| {
        let (red, green, blue) = layout.decode(source.get_pixel(x as u16, y as u16));
        xcap::image::Rgba([(red >> 8) as u8, (green >> 8) as u8, (blue >> 8) as u8, 255])
    });
    source.data_mut().zeroize();
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::{image::ColorComponent, protocol::xproto::CreateGCAux};

    #[test]
    fn native_x11_capture_matches_root_pixels() {
        if std::env::var("DEZIRCLIP_RUN_SCREENSHOT_SMOKE_TESTS").as_deref() != Ok("1") { return; }
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(std::env::var("RUNNER_ENVIRONMENT").as_deref(), Ok("github-hosted"));
        let (connection, screen_index) = x11rb::connect(None).unwrap();
        let screen = &connection.setup().roots[screen_index];
        let visual = screen.allowed_depths.iter().flat_map(|depth| &depth.visuals)
            .find(|visual| visual.visual_id == screen.root_visual).unwrap();
        let layout = PixelLayout::from_visual_type(*visual).unwrap();
        let gc = connection.generate_id().unwrap();
        connection.create_gc(gc, screen.root, &CreateGCAux::new().foreground(layout.encode((0xffff, 0x8080, 0)))).unwrap().check().unwrap();
        connection.poly_fill_rectangle(screen.root, gc, &[x11rb::protocol::xproto::Rectangle { x: 0, y: 0, width: screen.width_in_pixels, height: screen.height_in_pixels }]).unwrap().check().unwrap();
        connection.free_gc(gc).unwrap().check().unwrap();
        let full = super::super::capture_full_screen(None).unwrap();
        let frozen = super::super::freeze_at(None).unwrap();
        let crop = super::super::capture_region(7, 9, 101, 63).unwrap();
        let frozen_crop = frozen.crop(7, 9, 101, 63).unwrap();
        for (result, width, height) in [(&full, screen.width_in_pixels as u32, screen.height_in_pixels as u32), (&crop, 101, 63), (&frozen_crop, 101, 63)] {
            let pixels = xcap::image::load_from_memory(&result.png_bytes).unwrap().to_rgba8();
            assert_eq!(pixels.dimensions(), (width, height));
            assert_eq!(pixels.get_pixel(20, 20).0, [255, (layout.decode(layout.encode((0xffff, 0x8080, 0))).1 >> 8) as u8, 0, 255]);
        }
        println!("Verified X11 full, region and frozen capture at root depth {}", screen.root_depth);
    }

    #[test]
    fn rgb10_channels_preserve_range() {
        let layout = PixelLayout::new(ColorComponent::from_mask(0x3ff00000).unwrap(),
            ColorComponent::from_mask(0x000ffc00).unwrap(), ColorComponent::from_mask(0x000003ff).unwrap());
        assert_eq!(layout.decode(0x3fffffff), (65535, 65535, 65535));
        assert_eq!(layout.decode(0), (0, 0, 0));
    }
}
