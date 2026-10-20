//! Bounded image reads for explicitly selected appearance and managed emoji assets.
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageFormat, ImageReader};

const MAX_BYTES: u64 = 16 * 1024 * 1024;
const UNAVAILABLE: &str = "UserResourceUnavailable";

pub fn selected_background_path(data_dir: &Path, requested: &Path) -> Result<PathBuf, String> {
    let selected = requested.canonicalize().map_err(|_| UNAVAILABLE)?;
    let private_root = data_dir.canonicalize().map_err(|_| UNAVAILABLE)?;
    if selected.starts_with(private_root) {
        return managed_emoji_path(data_dir, requested);
    }
    Ok(selected)
}

pub fn managed_emoji_path(data_dir: &Path, requested: &Path) -> Result<PathBuf, String> {
    let root = data_dir.join("emoji_favorites");
    let metadata = std::fs::symlink_metadata(&root).map_err(|_| UNAVAILABLE)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() { return Err(UNAVAILABLE.into()); }
    let canonical_root = root.canonicalize().map_err(|_| UNAVAILABLE)?;
    let candidate = requested.canonicalize().map_err(|_| UNAVAILABLE)?;
    let name = candidate.file_name().and_then(|name| name.to_str()).ok_or(UNAVAILABLE)?;
    let (stem, extension) = name.rsplit_once('.').ok_or(UNAVAILABLE)?;
    let hash = stem.strip_prefix("fav_").ok_or(UNAVAILABLE)?;
    if candidate.parent() != Some(canonical_root.as_path()) || hash.is_empty() || hash.len() > 16
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !["png", "jpg", "jpeg", "gif", "webp"].contains(&extension) {
        return Err(UNAVAILABLE.into());
    }
    if std::fs::symlink_metadata(requested).map_err(|_| UNAVAILABLE)?.file_type().is_symlink() {
        return Err(UNAVAILABLE.into());
    }
    Ok(candidate)
}

fn image_data_url(bytes: &[u8]) -> Result<String, String> {
    let format = image::guess_format(bytes).map_err(|_| UNAVAILABLE)?;
    let mime = match format {
        ImageFormat::Png => "image/png", ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif", ImageFormat::WebP => "image/webp",
        _ => return Err(UNAVAILABLE.into()),
    };
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format).into_dimensions().map_err(|_| UNAVAILABLE)?;
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 || u64::from(width) * u64::from(height) > 40_000_000 {
        return Err(UNAVAILABLE.into());
    }
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

pub fn read_image(path: &Path) -> Result<String, String> {
    let file = crate::infrastructure::local_file::open_regular_nofollow(path).map_err(|_| UNAVAILABLE)?;
    let before = file.metadata().map_err(|_| UNAVAILABLE)?;
    if before.len() == 0 || before.len() > MAX_BYTES { return Err(UNAVAILABLE.into()); }
    let mut bytes = Vec::with_capacity(usize::try_from(before.len()).map_err(|_| UNAVAILABLE)?);
    (&file).take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|_| UNAVAILABLE)?;
    let after = file.metadata().map_err(|_| UNAVAILABLE)?;
    if bytes.len() as u64 != before.len() || before.len() != after.len()
        || before.modified().map_err(|_| UNAVAILABLE)? != after.modified().map_err(|_| UNAVAILABLE)? {
        return Err(UNAVAILABLE.into());
    }
    image_data_url(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("dzc-user-resource-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&path).unwrap(); Self(path)
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    fn png() -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 3).write_to(&mut output, ImageFormat::Png).unwrap();
        output.into_inner()
    }
    #[test]
    fn only_managed_direct_children_are_authorized() {
        let fixture = Fixture::new(); let root = fixture.0.join("emoji_favorites"); std::fs::create_dir(&root).unwrap();
        let good = root.join("fav_a123.png"); std::fs::write(&good, png()).unwrap();
        assert_eq!(managed_emoji_path(&fixture.0, &good).unwrap(), good.canonicalize().unwrap());
        for path in [fixture.0.join("fav_a123.png"), root.join("clipboard.png"), root.join("fav_z12.png")] {
            std::fs::write(&path, png()).unwrap(); assert!(managed_emoji_path(&fixture.0, &path).is_err());
        }
        assert!(managed_emoji_path(&fixture.0, &root.join("../fav_a123.png")).is_err());
    }
    #[test]
    fn actual_image_signature_is_required_and_file_bytes_are_bounded() {
        let fixture = Fixture::new(); let path = fixture.0.join("selected.png");
        std::fs::write(&path, b"<svg>untrusted markup</svg>").unwrap(); assert!(read_image(&path).is_err());
        std::fs::write(&path, png()).unwrap(); assert!(read_image(&path).unwrap().starts_with("data:image/png;base64,"));
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(MAX_BYTES + 1).unwrap();
        assert!(read_image(&path).is_err());
    }
    #[test]
    fn background_cannot_read_history_assets_through_settings() {
        let fixture = Fixture::new(); let data = fixture.0.join("data"); std::fs::create_dir(&data).unwrap();
        let selected = fixture.0.join("selected.png"); std::fs::write(&selected, png()).unwrap();
        assert_eq!(selected_background_path(&data, &selected).unwrap(), selected.canonicalize().unwrap());
        let history = data.join("attachments"); std::fs::create_dir(&history).unwrap();
        let secret = history.join("private.png"); std::fs::write(&secret, png()).unwrap();
        assert!(selected_background_path(&data, &secret).is_err());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn managed_symlinks_cannot_select_outside_images() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new(); let root = fixture.0.join("emoji_favorites"); std::fs::create_dir(&root).unwrap();
        let outside = fixture.0.join("outside.png"); std::fs::write(&outside, png()).unwrap();
        let link = root.join("fav_abc.png"); symlink(&outside, &link).unwrap();
        assert!(managed_emoji_path(&fixture.0, &link).is_err()); assert!(read_image(&link).is_err());
    }
}
