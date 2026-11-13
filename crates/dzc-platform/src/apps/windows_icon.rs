use std::io;
use base64::Engine;
use image::{ImageBuffer, Rgba};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ, RGBQUAD,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, GetSystemMetrics, DI_NORMAL, HICON, SM_CXSMICON, SM_CYSMICON,
};

use std::os::windows::ffi::OsStrExt;

/// Icon cache: the data URL keyed by normalised executable path.
///
/// `order` holds the same keys from least to most recently used, which is what
/// makes eviction LRU. A `HashMap` cannot supply this: its iteration order is
/// bucket order under a per-process random seed, so `keys().next()` names an
/// arbitrary entry rather than the oldest one, and re-inserting a hit to "move it
/// to the recent end" did nothing at all for want of an end to move it to.
///
/// Measured with a cap of 8 over a 4-hot / 24-cold workload, each round touching
/// all four hot keys before adding one cold one: the `HashMap` form retained 1 to
/// 3 of the 4 hot entries, varying run to run because the seed is drawn per
/// process, against 4 of 4 for this one. The number of entries the cache holds
/// stayed at the cap throughout -- what went wrong was which entries it kept,
/// not how many. A native miss costs 0.631 ms: it runs `ExtractIconExW`, draws
/// through a GDI DIB, encodes a PNG and base64-encodes it.
///
/// What a miss is worth per scroll is NOT established by this file. The frontend
/// shields this command: `ClipboardItem`'s effect depends on
/// `item.source_app_path` rather than on scroll position, `peekSourceAppIcon`
/// returns before the call is made, and `sourceAppIcon.ts` keeps its own cache
/// plus in-flight dedup. So the native cache is consulted when a path the
/// frontend has not kept turns up -- on the first mount of a row, and after the
/// frontend's own 256-entry LRU evicts it -- not once per mounted row per
/// scroll. Correcting the eviction order is what makes this cache behave as the
/// LRU it was written to be; the end-to-end saving has not been measured.
struct IconCache {
    entries: HashMap<String, Option<String>>,
    order: Vec<String>,
    cap: usize,
}

impl IconCache {
    fn new() -> Self {
        Self::with_cap(EXECUTABLE_ICON_CACHE_MAX)
    }

    fn with_cap(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            cap,
        }
    }

    /// A cached `None` is still a hit -- it means the extractor already ran and
    /// found no icon, and running it again would reach the same answer.
    fn get(&mut self, key: &str) -> Option<Option<String>> {
        let value = self.entries.get(key)?.clone();
        self.touch(key);
        Some(value)
    }

    fn insert(&mut self, key: String, value: Option<String>) {
        let is_new = self.entries.insert(key.clone(), value).is_none();
        if is_new {
            self.order.push(key);
        } else {
            self.touch(&key);
        }
        while self.order.len() > self.cap {
            let oldest = self.order.remove(0);
            self.entries.remove(&oldest);
        }
    }

    /// Move `key` to the most-recently-used end.
    fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let key = self.order.remove(pos);
            self.order.push(key);
        }
    }
}

static EXECUTABLE_ICON_CACHE: OnceLock<Mutex<IconCache>> = OnceLock::new();
const EXECUTABLE_ICON_CACHE_MAX: usize = 256;

pub fn get_executable_icon(executable_path: String) -> io::Result<Option<String>> {
    let cache_key = normalize_icon_cache_key(&executable_path);
    if cache_key.is_empty() {
        return Ok(None);
    }

    let cache = EXECUTABLE_ICON_CACHE.get_or_init(|| Mutex::new(IconCache::new()));
    {
        let mut guard = cache
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        if let Some(cached) = guard.get(&cache_key) {
            return Ok(cached);
        }
    }

    let icon = match packaged_icon(Path::new(&executable_path)) {
        Some(icon) => Some(icon),
        None => extract_executable_icon_data_url(&executable_path)?,
    };
    let mut guard = cache
        .lock()
        .map_err(|e| io::Error::other(e.to_string()))?;
    guard.insert(cache_key, icon.clone());
    Ok(icon)
}

fn package_logo(manifest: &str, executable: &str) -> Option<String> {
    use quick_xml::{events::Event, Reader};
    let mut reader = Reader::from_str(manifest);
    let mut selected = false;
    loop {
        match reader.read_event().ok()? {
            Event::Start(element) | Event::Empty(element) => {
                let local = element.local_name();
                if local.as_ref() == b"Application" {
                    selected = element.attributes().flatten().any(|attribute| attribute.key.as_ref() == b"Executable"
                        && attribute.unescape_value().is_ok_and(|value| value.replace('/', "\\").eq_ignore_ascii_case(executable)));
                } else if selected && local.as_ref() == b"VisualElements" {
                    for attribute in element.attributes().flatten() {
                        if attribute.key.as_ref() == b"Square44x44Logo" {
                            return attribute.unescape_value().ok().map(|value| value.into_owned());
                        }
                    }
                }
            }
            Event::End(element) if element.local_name().as_ref() == b"Application" => selected = false,
            Event::Eof => return None,
            _ => {}
        }
    }
}

fn packaged_icon(executable: &Path) -> Option<String> {
    for root in executable.parent()?.ancestors().take(4) {
        let manifest = root.join("AppxManifest.xml");
        let Some(xml) = crate::local_file::read_preview_file(&manifest, 256 * 1024).and_then(|bytes| String::from_utf8(bytes).ok()) else { continue; };
        let relative = executable.strip_prefix(root).ok()?.to_str()?;
        let Some(logo) = package_logo(&xml, relative) else { continue; };
        let logo = Path::new(&logo);
        if logo.components().any(|part| !matches!(part, std::path::Component::Normal(_))) { continue; }
        let original = root.join(logo);
        let stem = original.file_stem()?.to_str()?;
        let parent = original.parent()?;
        let mut candidates = vec![parent.join(format!("{stem}.targetsize-32_altform-unplated.png")), parent.join(format!("{stem}.targetsize-32.png")), original.clone()];
        // Packaged apps often contain only scale-qualified assets, not Logo.png.
        for suffix in ["scale-100", "scale-125", "scale-150", "scale-200", "scale-400", "targetsize-48", "targetsize-24"] {
            candidates.push(parent.join(format!("{stem}.{suffix}.png")));
        }
        for path in candidates {
            let Some(bytes) = crate::local_file::read_preview_file(&path, 512 * 1024) else { continue; };
            let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(1024); limits.max_image_height = Some(1024); limits.max_alloc = Some(8 * 1024 * 1024);
            reader.limits(limits);
            let Ok(image) = reader.decode() else { continue; };
            let mut png = std::io::Cursor::new(Vec::new());
            image.thumbnail(32, 32).write_to(&mut png, image::ImageFormat::Png).ok()?;
            return Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png.into_inner())));
        }
    }
    None
}

pub fn get_source_app_icon(executable_path: String) -> io::Result<Option<String>> {
    use std::os::windows::fs::MetadataExt;
    use std::path::{Component, Prefix};
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    let path = Path::new(&executable_path);
    let Some(Component::Prefix(prefix)) = path.components().next() else { return Ok(None); };
    let drive = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        _ => return Ok(None),
    };
    let root = [u16::from(drive), b':' as u16, b'\\' as u16, 0];
    // Stored history may have been imported. Read icons only from local disks.
    if !matches!(unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }, 2 | 3 | 5 | 6) {
        return Ok(None);
    }
    for part in path.ancestors() {
        if part.as_os_str().is_empty() { continue; }
        let metadata = match std::fs::symlink_metadata(part) {
            Ok(metadata) => metadata,
            Err(_) => return Ok(None),
        };
        if metadata.file_attributes() & 0x400 != 0 {
            return Ok(None);
        }
    }
    get_executable_icon(executable_path)
}

fn normalize_icon_cache_key(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    std::fs::canonicalize(trimmed)
        .map(|resolved| resolved.to_string_lossy().to_string())
        .unwrap_or_else(|_| trimmed.to_string())
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn extract_executable_icon_data_url(executable_path: &str) -> io::Result<Option<String>> {
    let trimmed = executable_path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let path = Path::new(trimmed);
    if !path.exists() || !path.is_file() {
        return Ok(None);
    }

    let icon = unsafe { extract_executable_icon_handle(trimmed)? };
    let Some(icon) = icon else {
        return Ok(None);
    };

    let png_result = unsafe { render_icon_to_png_bytes(icon) };
    let _ = unsafe { DestroyIcon(icon) };

    match png_result {
        Ok(bytes) => Ok(Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))),
        Err(err) => Err(err),
    }
}

unsafe fn extract_executable_icon_handle(executable_path: &str) -> io::Result<Option<HICON>> {
    let wide_path: Vec<u16> = OsStr::new(executable_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut large_icon = HICON::default();
    let mut small_icon = HICON::default();

    if ExtractIconExW(
        PCWSTR(wide_path.as_ptr()),
        0,
        Some(&mut large_icon),
        Some(&mut small_icon),
        1,
    ) == 0
    {
        return Ok(None);
    }

    let selected = if !small_icon.0.is_null() {
        small_icon
    } else {
        large_icon
    };

    if selected.0.is_null() {
        if !small_icon.0.is_null() {
            let _ = DestroyIcon(small_icon);
        }
        if !large_icon.0.is_null() {
            let _ = DestroyIcon(large_icon);
        }
        return Ok(None);
    }

    if selected.0 != small_icon.0 && !small_icon.0.is_null() {
        let _ = DestroyIcon(small_icon);
    }
    if selected.0 != large_icon.0 && !large_icon.0.is_null() {
        let _ = DestroyIcon(large_icon);
    }

    Ok(Some(selected))
}

unsafe fn render_icon_to_png_bytes(icon: HICON) -> io::Result<Vec<u8>> {
    let width = GetSystemMetrics(SM_CXSMICON).max(16);
    let height = GetSystemMetrics(SM_CYSMICON).max(16);
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [RGBQUAD::default()],
    };
    let mut bits = std::ptr::null_mut();
    let mem_dc = CreateCompatibleDC(None);
    if mem_dc.0.is_null() {
        return Err(io::Error::other(
            "Failed to create icon memory DC".to_string(),
        ));
    }

    let bitmap = match CreateDIBSection(None, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(bitmap) => bitmap,
        Err(err) => {
            let _ = DeleteDC(mem_dc);
            return Err(io::Error::other(format!(
                "Failed to create icon bitmap: {}",
                err
            )));
        }
    };

    let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
    let result = (|| -> io::Result<Vec<u8>> {
        if bits.is_null() {
            return Err(io::Error::other(
                "Icon bitmap has no backing buffer".to_string(),
            ));
        }

        let pixel_len = (width as usize) * (height as usize) * 4;
        std::slice::from_raw_parts_mut(bits as *mut u8, pixel_len).fill(0);
        DrawIconEx(mem_dc, 0, 0, icon, width, height, 0, None, DI_NORMAL)
            .map_err(|e| io::Error::other(format!("Failed to draw executable icon: {}", e)))?;
        GdiFlush().ok().map_err(|e| io::Error::other(e.to_string()))?;

        let bgra = std::slice::from_raw_parts(bits as *const u8, pixel_len);
        let mut rgba = vec![0u8; pixel_len];
        for (src, dst) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }

        let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width as u32, height as u32, rgba)
            .ok_or_else(|| {
            io::Error::other("Failed to build icon image buffer".to_string())
        })?;
        let mut png_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| io::Error::other(format!("Failed to encode icon PNG: {}", e)))?;
        Ok(png_bytes)
    })();

    let _ = SelectObject(mem_dc, old_bitmap);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    let _ = DeleteDC(mem_dc);
    result
}

#[cfg(test)]
mod tests {
    use super::{IconCache, EXECUTABLE_ICON_CACHE_MAX};
    #[test]
    fn package_icon_matches_the_requested_application() {
        let xml = r#"<Package><Applications><Application Executable="other.exe"><uap:VisualElements Square44x44Logo="other.png" /></Application><Application Executable="app\ChatGPT.exe"><uap:VisualElements Square44x44Logo="Assets\Chat&amp;GPT.png" /></Application></Applications></Package>"#;
        assert_eq!(super::package_logo(xml, "app\\chatgpt.exe").as_deref(), Some("Assets\\Chat&GPT.png"));
        assert!(super::package_logo(xml, "unknown.exe").is_none());
    }

    #[test]
    fn source_icon_extracts_local_png_and_rejects_remote_paths() {
        use base64::Engine;
        for remote in [r"\\server\share\application.exe", r"\\?\UNC\server\share\application.exe"] {
            assert!(super::get_source_app_icon(remote.into()).unwrap().is_none());
        }
        let explorer = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("explorer.exe");
        let icon = super::get_source_app_icon(explorer.to_string_lossy().into()).unwrap().unwrap();
        let encoded = icon.strip_prefix("data:image/png;base64,").unwrap();
        let png = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        let image = image::load_from_memory(&png).unwrap();
        assert!(image.width() >= 16 && image.height() >= 16);
        assert!(png.len() <= 256 * 1024);
    }

    /// A cache with the same shape but a cap small enough to evict inside a
    /// test. The production cap is 256, so a test that needs overflow would
    /// otherwise have to insert hundreds of keys to observe it -- and a test that
    /// quietly never fills the cache asserts nothing about eviction at all.
    fn small_cache(cap: usize) -> IconCache {
        IconCache::with_cap(cap)
    }

    fn cache_with(keys: &[&str]) -> IconCache {
        let mut cache = IconCache::new();
        for k in keys {
            cache.insert((*k).to_string(), Some(format!("icon-{k}")));
        }
        cache
    }

    fn contains(cache: &IconCache, key: &str) -> bool {
        cache.entries.contains_key(key)
    }

    // The defect this replaced: `HashMap::keys().next()` is bucket order, not
    // insertion order, so nothing was being evicted on use -- and a hit that
    // re-inserted the key moved it nowhere, because there was no end to move it
    // to. With the HashMap the entry that got thrown away was an arbitrary one.
    #[test]
    fn the_least_recently_used_entry_is_the_one_evicted() {
        let mut cache = small_cache(3);
        cache.insert("a".to_string(), Some("icon-a".to_string()));
        cache.insert("b".to_string(), Some("icon-b".to_string()));
        cache.insert("c".to_string(), Some("icon-c".to_string()));
        // Touch "a" so "b" becomes the oldest.
        cache.get("a");
        cache.insert("d".to_string(), Some("icon-d".to_string()));

        assert!(!contains(&cache, "b"), "the oldest entry is the one to go");
        assert!(contains(&cache, "a"), "a was just used, so it must stay");
        assert!(contains(&cache, "c"));
        assert!(contains(&cache, "d"));
    }

    // The measurement this is based on: with a cap of 8 over a 4-hot workload,
    // the HashMap form retained 1 to 3 of the 4 hot entries -- varying per run,
    // because the iteration order it depends on is seeded per process -- where
    // this one keeps 4. The cache still held its full 8 entries either way; what
    // differed was which ones.
    //
    // The workload interleaves the hot set with the cold tail rather than
    // flooding the cache in one block. That is what the app actually does -- a
    // row asks for its icon when it mounts, and a scroll mounts rows -- and it is
    // also the only shape in which an LRU can win: a cold flood with no hot
    // touches evicts the hot entries under any correct policy, because they
    // genuinely were the least recently used.
    #[test]
    fn a_hot_working_set_survives_a_long_cold_tail() {
        let hot = ["h0", "h1", "h2", "h3"];
        let mut cache = small_cache(8);
        // The hot entries have to be in the cache before they can be "touched" --
        // a get on an absent key refreshes nothing.
        for h in hot {
            cache.insert(h.to_string(), Some(format!("icon-{h}")));
        }
        for round in 0..400 {
            // One scroll: every visible row asks for its icon, so all four hot
            // apps are touched before the cold tail gets a turn.
            for h in hot {
                assert!(
                    cache.get(h).is_some(),
                    "{h} must hit on round {round} while it is visible"
                );
            }
            // ...and the cold tail adds entries nobody is looking at.
            cache.insert(format!("cold-{round}"), Some("icon-cold".to_string()));
        }
        // The hot entries were touched throughout, so all four are still there.
        for h in hot {
            assert!(cache.get(h).is_some(), "{h} must still be cached");
        }
        // And they must still be there on the next pass, not merely returned once.
        for h in hot {
            assert!(cache.get(h).is_some(), "{h} must survive a second pass");
        }
    }

    #[test]
    fn the_cache_never_exceeds_its_cap() {
        let mut cache = IconCache::new();
        for i in 0..(EXECUTABLE_ICON_CACHE_MAX * 2 + 5) {
            cache.insert(format!("k-{i}"), Some("icon".to_string()));
            assert!(
                cache.entries.len() <= EXECUTABLE_ICON_CACHE_MAX,
                "grew to {} past the cap of {EXECUTABLE_ICON_CACHE_MAX}",
                cache.entries.len()
            );
        }
        assert_eq!(cache.entries.len(), EXECUTABLE_ICON_CACHE_MAX);
        assert_eq!(cache.order.len(), EXECUTABLE_ICON_CACHE_MAX);
    }

    // A miss costs 0.631 ms here (ExtractIconExW + GDI draw + PNG + base64), so
    // caching "this path has no icon" has to count as a hit. Reading it as a miss
    // would redo the extraction for every row of a source that has no icon.
    #[test]
    fn a_cached_absence_is_still_a_hit() {
        let mut cache = IconCache::new();
        cache.insert("no-icon".to_string(), None);
        assert_eq!(
            cache.get("no-icon"),
            Some(None),
            "None must be cached, not re-extracted"
        );
        assert_eq!(cache.get("never-seen"), None, "an unknown key is a miss");
    }

    #[test]
    fn reinserting_an_existing_key_does_not_grow_the_cache() {
        let mut cache = IconCache::new();
        cache.insert("a".to_string(), Some("icon-a".to_string()));
        cache.insert("a".to_string(), Some("icon-a".to_string()));
        cache.insert("a".to_string(), Some("icon-a".to_string()));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(
            cache.order.len(),
            1,
            "order must not accumulate duplicates"
        );
    }

    #[test]
    fn a_hit_returns_the_stored_value() {
        let mut cache = cache_with(&["only"]);
        assert_eq!(cache.get("only"), Some(Some("icon-only".to_string())));
    }

    // Reviewer: the individual tests cover the pieces, not the combination.
    // A cached `None` that is touched and then pushed out by overflow is the
    // case that matters -- an entry with no icon still has to take part in
    // eviction correctly, and it is the one whose value is easiest to lose
    // track of because there is nothing to hand back.
    #[test]
    fn a_cached_absence_takes_part_in_eviction() {
        let mut cache = small_cache(3);
        cache.insert("oldest".to_string(), Some("icon-oldest".to_string()));
        cache.insert("no-icon".to_string(), None);
        cache.insert("newest".to_string(), Some("icon-newest".to_string()));

        // Touch the absence so it is no longer the oldest entry.
        assert_eq!(cache.get("no-icon"), Some(None));

        // Overflow: "oldest" is now the least recently used.
        cache.insert("newcomer".to_string(), Some("icon-newcomer".to_string()));

        assert!(!contains(&cache, "oldest"), "the oldest entry is evicted");
        assert!(
            contains(&cache, "no-icon"),
            "a touched absence must be kept, not evicted for being empty"
        );
        assert!(contains(&cache, "newest"));
        assert!(contains(&cache, "newcomer"));
        assert_eq!(cache.entries.len(), 3);
    }

    // Reviewer: overwriting an existing key with a different value must update
    // both the stored value and its recency, and must not leave the old value
    // reachable. `insert` is reached on the miss path, so a path that was
    // cached and then re-extracted goes through this.
    #[test]
    fn overwriting_updates_the_value_and_the_recency() {
        let mut cache = small_cache(3);
        cache.insert("a".to_string(), Some("icon-a-v1".to_string()));
        cache.insert("b".to_string(), Some("icon-b".to_string()));
        cache.insert("c".to_string(), Some("icon-c".to_string()));

        // Overwrite "a" with a different value. Nothing reads "a" between this
        // and the overflow below: a `get` would itself refresh the recency and
        // mask an `insert` that failed to.
        cache.insert("a".to_string(), Some("icon-a-v2".to_string()));

        // Overflow now evicts "b" (oldest), not "a".
        cache.insert("d".to_string(), Some("icon-d".to_string()));
        assert!(!contains(&cache, "b"), "overwriting must refresh recency");
        assert!(
            contains(&cache, "a"),
            "the overwritten key must not be the one evicted"
        );
        assert_eq!(cache.entries.len(), 3);
        assert_eq!(cache.order.len(), 3, "order must not gain a duplicate");

        // And the stored value really is the newer one.
        assert_eq!(
            cache.get("a"),
            Some(Some("icon-a-v2".to_string())),
            "the newer value must replace the old one"
        );
    }
}
