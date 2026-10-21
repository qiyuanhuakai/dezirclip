use std::collections::VecDeque;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use crate::infrastructure::image_preview_file::{open_regular_file, PreviewFile};

const PREVIEW_EDGE: u32 = 480;
const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RAW_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 16_384;
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 24;
const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;

// Filesystem work must not hold either mutex: a stalled source cannot prevent
// other requests from using the cache or decoding a different image.
static PREVIEW_CACHE: OnceLock<Mutex<PreviewCache>> = OnceLock::new();
static PREVIEW_DECODE: Mutex<()> = Mutex::new(());
static RAW_INPUT_BUDGET: RawInputBudget = RawInputBudget::new(MAX_RAW_INPUT_BYTES);

struct RawInputBudget {
    used: AtomicU64,
    limit: u64,
}

struct RawInputReservation<'a> {
    budget: &'a RawInputBudget,
    bytes: u64,
}

impl RawInputBudget {
    const fn new(limit: u64) -> Self {
        Self { used: AtomicU64::new(0), limit }
    }

    fn try_reserve(&self, bytes: u64) -> Option<RawInputReservation<'_>> {
        self.used.fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            used.checked_add(bytes).filter(|total| *total <= self.limit)
        }).ok().map(|_| RawInputReservation { budget: self, bytes })
    }
}

impl Drop for RawInputReservation<'_> {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[derive(Clone, PartialEq, Eq)]
struct PreviewKey {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

struct CachedPreview {
    key: PreviewKey,
    data_url: String,
    heap_bytes: usize,
}

struct PreviewCache {
    entries: VecDeque<CachedPreview>,
    heap_bytes: usize,
}

impl Default for PreviewCache {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_CACHE_ENTRIES),
            heap_bytes: 0,
        }
    }
}

impl PreviewCache {
    fn budget(&self) -> usize {
        MAX_CACHE_BYTES.saturating_sub(
            self.entries.capacity() * std::mem::size_of::<CachedPreview>()
                + std::mem::size_of::<Self>(),
        )
    }

    fn get(&mut self, key: &PreviewKey) -> Option<String> {
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        let entry = self.entries.remove(index)?;
        let result = entry.data_url.clone();
        self.entries.push_front(entry);
        Some(result)
    }

    fn insert(&mut self, key: PreviewKey, data_url: String) {
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            if let Some(entry) = self.entries.remove(index) {
                self.heap_bytes -= entry.heap_bytes;
            }
        }
        let heap_bytes = key.path.capacity().saturating_add(data_url.capacity());
        let budget = self.budget();
        if heap_bytes > budget {
            return;
        }
        while self.entries.len() >= MAX_CACHE_ENTRIES
            || self.heap_bytes.saturating_add(heap_bytes) > budget
        {
            let Some(entry) = self.entries.pop_back() else {
                break;
            };
            self.heap_bytes -= entry.heap_bytes;
        }
        self.heap_bytes += heap_bytes;
        self.entries.push_front(CachedPreview {
            key,
            data_url,
            heap_bytes,
        });
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err("Image dimensions exceed the preview limit".to_string());
    }
    Ok(())
}

fn decode_limited(mut decoder: impl ImageDecoder) -> Result<(DynamicImage, Orientation), String> {
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height)?;
    if decoder.total_bytes() > MAX_DECODE_BYTES {
        return Err("Decoded image exceeds the preview limit".to_string());
    }
    let orientation = decoder.orientation().map_err(|err| err.to_string())?;
    DynamicImage::from_decoder(decoder)
        .map(|image| (image, orientation))
        .map_err(|err| format!("Failed to decode preview: {err}"))
}

fn encode_preview(image: DynamicImage, orientation: Orientation) -> Result<String, String> {
    let mut preview = if image.width() > PREVIEW_EDGE || image.height() > PREVIEW_EDGE {
        image.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE)
    } else {
        image
    };
    // Apply camera orientation after shrinking, avoiding a second full-size copy.
    preview.apply_orientation(orientation);
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(preview.to_rgba8())
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|err| format!("Failed to encode preview: {err}"))?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(encoded.into_inner())))
}

fn decode_preview(bytes: Vec<u8>) -> Result<String, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| format!("Failed to identify preview image: {err}"))?;
    let format = reader.format().ok_or_else(|| "Unknown image format".to_string())?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp) {
        return Err("Preview is limited to static PNG, JPEG and BMP images".to_string());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let (image, orientation) = if format == ImageFormat::Png {
        let decoder = image::codecs::png::PngDecoder::with_limits(reader.into_inner(), limits)
            .map_err(|err| format!("Failed to read PNG preview: {err}"))?;
        if decoder.is_apng().map_err(|err| err.to_string())? {
            return Err("Animated PNG images retain their original preview".to_string());
        }
        decode_limited(decoder)?
    } else {
        reader.limits(limits);
        let decoder = reader.into_decoder().map_err(|err| err.to_string())?;
        decode_limited(decoder)?
    };
    encode_preview(image, orientation)
}

fn cached_preview(key: &PreviewKey) -> Result<Option<String>, String> {
    PREVIEW_CACHE
        .get_or_init(|| Mutex::new(PreviewCache::default()))
        .lock()
        .map_err(|err| format!("Preview cache lock failed: {err}"))
        .map(|mut cache| cache.get(key))
}

fn load_preview_with_reader(
    path: &Path,
    read: impl FnOnce(PreviewFile) -> Result<Vec<u8>, String>,
) -> Result<String, String> {
    let source = open_regular_file(path, MAX_INPUT_BYTES)
        .map_err(|err| format!("Failed to open preview image: {err}"))?;
    let key = PreviewKey {
        path: source.canonical_path.clone(),
        size: source.metadata.len(),
        modified: source.metadata.modified().map_err(|err| err.to_string())?,
    };
    if let Some(preview) = cached_preview(&key)? {
        return Ok(preview);
    }
    // Fail immediately rather than queueing unbounded compressed-image buffers.
    // The reservation survives the read and any wait for the single decoder.
    let _raw_input = RAW_INPUT_BUDGET.try_reserve(key.size)
        .ok_or_else(|| "Image preview input budget is busy".to_string())?;
    let bytes = read(source)?;
    let _decode = PREVIEW_DECODE.lock()
        .map_err(|err| format!("Preview decoder lock failed: {err}"))?;
    if let Some(preview) = cached_preview(&key)? {
        return Ok(preview);
    }
    let preview = decode_preview(bytes)?;
    PREVIEW_CACHE.get_or_init(|| Mutex::new(PreviewCache::default()))
        .lock().map_err(|err| format!("Preview cache lock failed: {err}"))?
        .insert(key, preview.clone());
    Ok(preview)
}

fn load_preview(path: &Path) -> Result<String, String> {
    load_preview_with_reader(path, |source| source.read_checked()
        .map_err(|err| format!("Failed to read preview image: {err}")))
}

#[tauri::command]
pub async fn get_image_preview(_path: String) -> Result<String, String> {
    Err("PrivacyResourceIdRequired".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn key(index: usize) -> PreviewKey {
        PreviewKey {
            path: PathBuf::from(format!("image-{index}.png")),
            size: 100,
            modified: UNIX_EPOCH,
        }
    }

    #[test]
    fn preview_is_a_png_with_bounded_size_and_preserved_aspect_ratio() {
        let preview = encode_preview(DynamicImage::new_rgba8(1920, 1080), Orientation::NoTransforms).unwrap();
        let bytes = STANDARD.decode(preview.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (480, 270));
    }

    #[test]
    fn preview_preserves_camera_orientation_without_upscaling() {
        let preview = encode_preview(DynamicImage::new_rgb8(96, 48), Orientation::Rotate90).unwrap();
        let bytes = STANDARD.decode(preview.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (48, 96));
    }

    #[test]
    fn oversized_pixel_counts_and_dimensions_are_rejected() {
        assert!(validate_dimensions(3840, 2160).is_ok());
        assert!(validate_dimensions(8192, 8192).is_err());
        assert!(validate_dimensions(MAX_IMAGE_DIMENSION + 1, 1).is_err());
        assert!(validate_dimensions(0, 10).is_err());
    }

    #[test]
    fn cache_evicts_the_least_recently_used_entry_at_its_item_limit() {
        let mut cache = PreviewCache::default();
        for index in 0..MAX_CACHE_ENTRIES {
            cache.insert(key(index), index.to_string());
        }
        assert_eq!(cache.get(&key(0)), Some("0".to_string()));
        cache.insert(key(MAX_CACHE_ENTRIES), "new".to_string());
        assert!(cache.get(&key(1)).is_none());
        assert!(cache.get(&key(0)).is_some());
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
    }

    #[test]
    fn cache_bounds_allocations_and_invalidates_changed_metadata() {
        let mut cache = PreviewCache::default();
        cache.insert(key(0), "x".repeat(MAX_CACHE_BYTES / 2));
        cache.insert(key(1), "y".repeat(MAX_CACHE_BYTES / 2));
        assert!(cache.get(&key(0)).is_none());
        assert!(cache.heap_bytes <= cache.budget());
        let mut changed = key(1);
        changed.modified += Duration::from_secs(1);
        assert!(cache.get(&changed).is_none());
        changed = key(1);
        changed.size += 1;
        assert!(cache.get(&changed).is_none());
    }

    #[test]
    fn raw_input_budget_refuses_excess_and_reuses_capacity_after_drop() {
        let budget = RawInputBudget::new(8);
        let first = budget.try_reserve(6).unwrap();
        assert!(budget.try_reserve(3).is_none());
        let second = budget.try_reserve(2).unwrap();
        assert_eq!(budget.used.load(Ordering::Acquire), 8);
        assert!(budget.try_reserve(1).is_none());
        drop(first);
        let replacement = budget.try_reserve(6).unwrap();
        assert_eq!(budget.used.load(Ordering::Acquire), 8);
        drop(second);
        drop(replacement);
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn raw_input_budget_rejects_counter_overflow_without_leaking_capacity() {
        let budget = RawInputBudget::new(u64::MAX);
        let reservation = budget.try_reserve(u64::MAX).unwrap();
        assert!(budget.try_reserve(1).is_none());
        assert_eq!(budget.used.load(Ordering::Acquire), u64::MAX);
        drop(reservation);
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
        assert!(budget.try_reserve(1).is_some());
    }

    #[test]
    fn a_stalled_source_read_does_not_block_a_normal_preview() {
        use std::sync::mpsc;

        let directory = std::env::temp_dir().join(format!(
            "dezirclip-preview-concurrency-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        std::fs::create_dir(&directory).unwrap();
        let first_path = directory.join("stalled.png");
        let second_path = directory.join("normal.png");
        let mut image = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(2, 2).write_to(&mut image, ImageFormat::Png).unwrap();
        std::fs::write(&first_path, image.get_ref()).unwrap();
        std::fs::write(&second_path, image.get_ref()).unwrap();
        let (entered, entered_receiver) = mpsc::channel();
        let (resume, resume_receiver) = mpsc::channel();
        let stalled = std::thread::spawn(move || load_preview_with_reader(&first_path, |source| {
            entered.send(()).map_err(|err| err.to_string())?;
            resume_receiver.recv_timeout(Duration::from_secs(5)).map_err(|err| err.to_string())?;
            source.read_checked().map_err(|err| err.to_string())
        }));
        entered_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let (completed, completed_receiver) = mpsc::channel();
        let normal = std::thread::spawn(move || {
            completed.send(load_preview(&second_path)).unwrap();
        });
        let independent_result = completed_receiver.recv_timeout(Duration::from_secs(2));
        let _ = resume.send(());
        let stalled_result = stalled.join().unwrap();
        normal.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        assert!(independent_result.unwrap().unwrap().starts_with("data:image/png;base64,"));
        assert!(stalled_result.is_ok());
    }
}
