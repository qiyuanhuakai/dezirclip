use rusqlite::{Connection, Result};

use base64::Engine;
use std::borrow::Cow;

pub use crate::infrastructure::encryption::{self, ENCRYPT_PREFIX};

pub use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository;
use crate::infrastructure::repository::settings_repo::SqliteSettingsRepository;
use crate::infrastructure::repository::tag_repo::SqliteTagRepository;
use std::sync::{Arc, Mutex};

pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
    pub repo: SqliteClipboardRepository,
    pub settings_repo: SqliteSettingsRepository,
    pub tag_repo: SqliteTagRepository,
}

const SENSITIVE_KEYS: &[&str] = &[];

pub const SENSITIVE_TAGS: &[&str] = &["sensitive", "密码"];

pub fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

pub fn has_sensitive_tag(tags: &[String]) -> bool {
    tags.iter()
        .any(|t| SENSITIVE_TAGS.iter().any(|s| s.eq_ignore_ascii_case(t)))
}

pub fn is_text_type(content_type: &str) -> bool {
    matches!(content_type, "text" | "code" | "url" | "rich_text")
}

/// The clipboard treats `\r\n` and `\n` as the same content everywhere it
/// compares or hashes text, so every capture and every paste normalises before
/// doing so. `str::replace` always allocates, and it was doing that on the whole
/// payload — on a 10 MB text entry that is a full copy per event, and the
/// session dedup scan repeats it once per remembered item. Normalised clipboard
/// text is almost never CRLF, so hand back a borrow in that case and only
/// allocate when there is genuinely a `\r\n` to fold.
pub fn fold_crlf(content: &str) -> Cow<'_, str> {
    if content.contains("\r\n") {
        Cow::Owned(content.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(content)
    }
}

/// `content` trimmed and CRLF-folded, matching what the history comparison has
/// always treated as equal.
pub fn normalize_text(content: &str) -> Cow<'_, str> {
    fold_crlf(content.trim())
}

pub fn calc_text_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let normalized = normalize_text(content);
    let mut hasher = DefaultHasher::new();    normalized.hash(&mut hasher);
    hasher.finish()
}

/// How much of the base64 payload to decode purely to name the image format.
///
/// 32 base64 characters are 24 bytes, which covers every signature this build
/// supports: PNG needs 8, GIF 6, BMP 2, JPEG 3, WebP 12. Must stay a multiple
/// of four so the quantum is whole.
const SNIFF_BASE64_LEN: usize = 32;

/// Ceiling on the decoded size of an image taken from the clipboard.
///
/// 8K RGBA is about 132 MB, so this leaves room for anything a screenshot or a
/// copied photo can legitimately be while still rejecting the header-only
/// bombs that a few kilobytes of PNG can otherwise expand into gigabytes.
pub const MAX_IMAGE_DECODE_BYTES: u64 = 256 * 1024 * 1024;

/// Hashes the 32x32 nearest-neighbour thumbnail of an already decoded image.
///
/// Both entry points need this exact tail: `calc_image_hash` for `data:` URLs
/// and the repository's on-disk branch for a content that is a file path. The two
/// have to agree — `content_hash` is the column the dedup lookup matches on, so a
/// divergence would hash an image one way on the way in and another on the way to
/// the database, and the picture would never deduplicate.
pub fn thumbnail_hash(img: &image::DynamicImage) -> i64 {
    let thumb = img.resize_exact(32, 32, image::imageops::FilterType::Nearest);
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    thumb.as_bytes().hash(&mut hasher);
    hasher.finish() as i64
}

pub fn calc_image_hash(base64_data: &str) -> Option<i64> {
    // `split_once` gives the same payload as `splitn(2, ',').nth(1)` without
    // allocating a Vec for the two halves.
    let payload = match base64_data.split_once(',') {
        Some((_, rest)) => rest,
        None => base64_data,
    };

    // The payload of a screenshot data URL runs into megabytes. Only allocate a
    // cleaned copy when there is whitespace to strip at all, and strip it in one
    // pass instead of `replace("\r", "").replace("\n", "")`, which made two
    // full-size copies on every lookup.
    let cleaned;
    let payload = if payload.contains(['\r', '\n']) {
        cleaned = payload
            .chars()
            .filter(|c| *c != '\r' && *c != '\n')
            .collect::<String>();
        cleaned.as_str()
    } else {
        payload
    };

    use base64::Engine;
    let payload = payload.trim();
    let engine = base64::engine::general_purpose::STANDARD;
    let bytes = payload.as_bytes();

    // A decodable header is not the same thing as a decodable picture: a
    // 30x30 pixel file can expand to more memory than a 4K screenshot. Naming
    // the format from a single short quantum — every format here announces
    // itself in the first 24 bytes — lets the decode run under an explicit
    // limit, so a crafted "decompression bomb" comes back as a clean `None`
    // instead of an allocation the process cannot satisfy.
    let head_len = bytes.len().min(SNIFF_BASE64_LEN);
    let format = image::guess_format(&engine.decode(&bytes[..head_len]).ok()?).ok()?;

    let decoded = engine.decode(bytes).ok()?;

    // The limit sits far above any real clipboard image — 8K RGBA is ~132 MB —
    // so ordinary screenshots and photos are untouched.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_IMAGE_DECODE_BYTES);

    // Decoding out of a `Cursor` rather than `load_from_memory` is what makes
    // the limit reachable: the reader type that can carry limits is bounded on
    // `Seek`. It also means the compressed bytes are handed to the decoder and
    // then released before the thumbnail is resized, instead of both sitting
    // on the heap for the whole call.
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&decoded), format);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    drop(decoded);

    Some(thumbnail_hash(&img))
}

pub fn init_db(path: &str) -> Result<Connection> {
    fn init_db_once(path: &str) -> Result<Connection> {
        let mut conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA auto_vacuum = FULL;
        ",
        )?;
        crate::infrastructure::repository::migrations::run_migrations(&mut conn)?;
        seed_defaults(&conn)?;
        Ok(conn)
    }

    fn is_disk_io_error(err: &rusqlite::Error) -> bool {
        err.to_string()
            .to_ascii_lowercase()
            .contains("disk i/o error")
    }

    fn quarantine_corrupted_db(path: &str) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let db_path = std::path::PathBuf::from(path);
        if !db_path.exists() {
            return;
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let base = db_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("clipboard");
        let ext = db_path.extension().and_then(|s| s.to_str()).unwrap_or("db");
        let mut backup = db_path.clone();
        backup.set_file_name(format!("{base}.corrupt-{ts}.{ext}"));
        let _ = std::fs::rename(&db_path, &backup);

        let mut wal_path = db_path.clone();
        wal_path.set_file_name(format!(
            "{}-wal",
            db_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("clipboard.db")
        ));
        if wal_path.exists() {
            let mut wal_backup = backup.clone();
            wal_backup.set_file_name(format!(
                "{}-wal",
                backup
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("clipboard.corrupt.db")
            ));
            let _ = std::fs::rename(&wal_path, &wal_backup);
        }

        let mut shm_path = db_path.clone();
        shm_path.set_file_name(format!(
            "{}-shm",
            db_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("clipboard.db")
        ));
        if shm_path.exists() {
            let mut shm_backup = backup.clone();
            shm_backup.set_file_name(format!(
                "{}-shm",
                backup
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("clipboard.corrupt.db")
            ));
            let _ = std::fs::rename(&shm_path, &shm_backup);
        }
    }

    match init_db_once(path) {
        Ok(conn) => Ok(conn),
        Err(err) if is_disk_io_error(&err) => {
            quarantine_corrupted_db(path);
            init_db_once(path)
        }
        Err(err) => Err(err),
    }
}

// save_entry removed (migrated to repository)

pub fn save_image_to_file(data_url: &str, data_dir: &std::path::Path) -> Option<String> {
    use std::io::Write;
    let parts: Vec<&str> = data_url.splitn(2, ',').collect();
    if parts.len() < 2 {
        return None;
    }

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(parts[1])
        .ok()?;

    let attachments_dir = data_dir.join("attachments");
    if !attachments_dir.exists() {
        let _ = std::fs::create_dir_all(&attachments_dir);
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    decoded.hash(&mut hasher);
    let hash = hasher.finish();

    let file_name = format!("img_{:x}.png", hash);
    let file_path = attachments_dir.join(&file_name);

    if !file_path.exists() {
        let mut file = std::fs::File::create(&file_path).ok()?;
        file.write_all(&decoded).ok()?;
    }

    Some(file_path.to_string_lossy().to_string())
}

// get_history removed (migrated to repository)

// search_history removed (migrated to repository)

// get_important_items removed (migrated to repository)

// delete_entry_db, delete_entry, enforce_storage_limit, clear_history removed (migrated to repository)

pub fn seed_defaults(conn: &Connection) -> Result<()> {
    // App settings
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.theme', 'mica')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.color_mode', 'system')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.show_app_border', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.persistent', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.capture_files', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.capture_rich_text', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.rich_text_snapshot_preview', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.deduplicate', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.silent_start', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.delete_after_paste', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.move_to_top_after_paste', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.privacy_protection', 'true')",
        [],
    );
    let _ = conn.execute("INSERT OR IGNORE INTO settings (key, value) VALUES ('app.privacy_protection_kinds', 'phone,idcard,email,secret,password')", []);
    let _ = conn.execute("INSERT OR IGNORE INTO settings (key, value) VALUES ('app.privacy_protection_custom_rules', '')", []);
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.use_win_v_shortcut', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.sequential_mode', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.sequential_hotkey', 'Alt+V')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.rich_paste_hotkey', 'Ctrl+Shift+Z')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.search_hotkey', 'Alt+F')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.screenshot_enabled', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.screenshot_hotkey', 'Ctrl+Shift+A')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.quick_paste_enabled', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.quick_paste_hotkey', 'Ctrl+Shift+V')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.ocr_enabled', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.sound_enabled', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.sound_paste_enabled', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.hide_tray_icon', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.edge_docking', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.follow_mouse', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.disable_webview_gpu', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.arrow_key_selection', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.window_pinned', 'false')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.hotkey', 'Alt+C')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.autostart', 'true')",
        [],
    );
    let _ = conn.execute("INSERT OR IGNORE INTO settings (key, value) VALUES ('app.win_clipboard_disabled', 'false')", []);
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.custom_background', '')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.custom_background_opacity', '45')",
        [],
    );
    // Absolute shell alpha (percent), not a multiplier on a per-theme base, so
    // the same value reads identically in every theme and colour mode.
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.surface_opacity', '65')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.notice_v028_shown', 'true')",
        [],
    );

    // Paste method setting: "shift_insert" (default) or "ctrl_v"
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.paste_method', 'shift_insert')",
        [],
    );

    // Storage limit settings
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.persistent_limit_enabled', 'true')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.persistent_limit', '500')",
        [],
    );

    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.font_main', '')",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('app.font_mono', '')",
        [],
    );

    Ok(())
}

// Migrated to repositories: toggle_pin, update_pinned_order, get_entry_by_content,
// update_entry_content, insert_entry, get_entry_content, get_entry_content_full,
// get_entry_content_with_html, get_entry_by_id, update_entry_tags, get_all_tags,
// create_tag, rename_tag, delete_tag_globally, get_entries_by_tag, set_tag_color, get_tag_colors

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::repository::clipboard_repo::{
        ClipboardRepository, SqliteClipboardRepository,
    };
    use crate::infrastructure::repository::settings_repo::{
        SettingsRepository, SqliteSettingsRepository,    };

    // 辅助函数：创建一个内存中的临时测试数据库
    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // 调用你的 init_db 逻辑（手动执行部分关键建表语句）
        conn.execute(
            "CREATE TABLE clipboard_history (
                id INTEGER PRIMARY KEY,
                content_type TEXT NOT NULL,
                content TEXT NOT NULL,
                html_content TEXT,
                source_app TEXT NOT NULL,
                source_app_path TEXT,
                timestamp INTEGER NOT NULL,
                preview TEXT NOT NULL,
                is_pinned INTEGER NOT NULL DEFAULT 0,
                content_hash INTEGER NOT NULL DEFAULT 0,
                tags TEXT NOT NULL DEFAULT '[]',
                use_count INTEGER NOT NULL DEFAULT 0,
                is_external INTEGER NOT NULL DEFAULT 0,
                pinned_order INTEGER NOT NULL DEFAULT 0,
                ocr_text TEXT,
                ocr_status TEXT NOT NULL DEFAULT 'pending'
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE entry_tags (
                entry_id INTEGER NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (entry_id, tag)
            )",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_save_and_get_history() {
        let conn = setup_test_db();

        let entry = ClipboardEntry {
            id: 0,
            content_type: "text".to_string(),
            content: "Hello Integration Test".to_string(),
            html_content: None,
            source_app: "TestApp".to_string(),
            source_app_path: Some("C:\\TestApp.exe".to_string()),
            timestamp: 123456789,
            preview: "Hello...".to_string(),
            is_pinned: false,
            tags: vec![],
            use_count: 0,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: true,
            content_kinds: Vec::new(),
            ocr_text: None,
            ocr_status: None,
        };

        let conn_arc = Arc::new(Mutex::new(conn));
        let repo = SqliteClipboardRepository::new(conn_arc);

        // 1. 测试保存
        let id = repo.save(&entry, None).expect("保存失败");
        assert!(id > 0);

        // 2. 测试获取
        let history = repo.get_history(10, 0, None).expect("获取历史失败");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "Hello Integration Test");
        assert_eq!(history[0].source_app, "TestApp");
    }

    #[test]
    fn test_settings_persistence() {
        let conn = setup_test_db();
        let conn_arc = Arc::new(Mutex::new(conn));
        let repo = SqliteSettingsRepository::new(conn_arc);

        // 测试设置保存
        repo.set("test_key", "test_value").unwrap();

        // 测试设置读取
        let val = repo.get("test_key").unwrap();
        assert_eq!(val, Some("test_value".to_string()));
    }

    // 1x1 opaque PNG.
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn tiny_png_data_url() -> String {
        use base64::Engine;
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(TINY_PNG)
        )
    }

    // The dedup pipeline hashes the image once and reuses that hash for a second
    // lookup whose input differs only by trimmed / CRLF-folded whitespace. If
    // the whitespace handling ever drifts, the two lookups would disagree and
    // the app would stop recognising its own duplicates.
    #[test]
    fn image_hash_ignores_surrounding_and_folded_whitespace() {
        let url = tiny_png_data_url();
        let base = calc_image_hash(&url).expect("tiny png should hash");

        let mut noisy = String::new();
        noisy.push_str("  \r\n");
        for chunk in url.as_bytes().chunks(24) {
            noisy.push_str(std::str::from_utf8(chunk).unwrap());
            noisy.push_str("\r\n");
        }
        noisy.push_str("\n  ");

        assert_eq!(calc_image_hash(&noisy), Some(base));
    }

    #[test]
    fn image_hash_rejects_non_image_payloads() {
        assert_eq!(calc_image_hash("data:image/png;base64,bm90IGFuIGltYWdl"), None);
        assert_eq!(calc_image_hash(""), None);
    }

    // The data-URL path and the on-disk path are two callers of one helper, and
    // they have to land on the same number: `content_hash` is both what the
    // dedup lookup matches on and what the row is stored with, so a divergence
    // would make an image look like a new picture every time it was pasted.
    #[test]
    fn thumbnail_hash_matches_the_data_url_path() {
        use base64::Engine;
        let url = tiny_png_data_url();
        let payload = url.split_once(',').expect("data url has a payload").1;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("payload decodes");
        let decoded_image = image::load_from_memory(&decoded).expect("payload decodes to image");

        assert_eq!(
            calc_image_hash(&url),
            Some(thumbnail_hash(&decoded_image))
        );
    }

    // The same picture reached through two routes must hash identically, since
    // one arrives as a data URL and the other as a file on disk.
    #[test]
    fn thumbnail_hash_ignores_how_the_bytes_arrived() {
        use base64::Engine;
        let url = tiny_png_data_url();
        let payload = url.split_once(',').expect("data url has a payload").1;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("payload decodes");

        let from_url = calc_image_hash(&url).expect("tiny png should hash");
        let from_path = image::load_from_memory(&decoded)
            .map(|img| thumbnail_hash(&img))
            .expect("same bytes decode the same way");

        assert_eq!(from_url, from_path);
    }

    // Distinct pictures must not collide, otherwise dedup would drop a real
    // clipboard entry. A 1x1 red PNG against the reference tiny PNG is enough:
    // different pixels, same dimensions, so only the pixel hash separates them.
    #[test]
    fn thumbnail_hash_separates_different_pixels() {
        use base64::Engine;
        let url = tiny_png_data_url();
        let payload = url.split_once(',').expect("data url has a payload").1;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("payload decodes");
        let reference = image::load_from_memory(&decoded).expect("payload decodes to image");
        let recolored = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([255, 0, 0, 255]),
        ));

        assert_ne!(thumbnail_hash(&reference), thumbnail_hash(&recolored));
    }

    // The borrow arm is the whole point: normalised clipboard text is normally
    // LF already, and copying it anyway is what this change removes. A silent
    // switch back to always-allocating would still pass every equality test
    // below, so the allocation itself has to be asserted.
    #[test]
    fn fold_crlf_borrows_when_there_is_nothing_to_fold() {
        assert!(matches!(fold_crlf("plain lf text"), Cow::Borrowed(_)));
        assert!(matches!(fold_crlf(""), Cow::Borrowed(_)));
        assert!(matches!(fold_crlf("lone \r carriage"), Cow::Borrowed(_)));
        assert!(matches!(fold_crlf("windows\r\nline\r\nend"), Cow::Owned(_)));
    }

    // Every consumer compares or hashes this value, so it has to match the
    // `trim().replace("\r\n", "\n")` it replaced, character for character.
    #[test]
    fn normalize_text_matches_the_string_it_replaced() {
        let reference = |s: &str| s.trim().replace("\r\n", "\n");

        for input in [
            "",
            "   ",
            "plain",
            "  padded  ",
            "a\r\nb",
            "a\r\nb\nc",
            "\r\nleading crlf",
            "trailing crlf\r\n",
            "mixed \r\n and \n and \r",
            "\u{4f60}\u{597d}\r\n\u{4e16}\u{754c}",
            "emoji \u{1f389}\r\ntail",
        ] {
            assert_eq!(
                normalize_text(input),
                reference(input),
                "normalize_text diverged for {input:?}"
            );
        }
    }

    // The hash is what the clipboard monitor uses to recognise its own writes.
    // If the folded and borrowed arms ever hashed differently, the app would
    // stop suppressing its own echoes and re-capture everything it pastes.
    #[test]
    fn text_hash_is_stable_across_line_endings() {
        assert_eq!(
            calc_text_hash("line one\r\nline two"),
            calc_text_hash("line one\nline two")
        );
        assert_eq!(calc_text_hash("  padded  "), calc_text_hash("padded"));
        assert_ne!(calc_text_hash("alpha"), calc_text_hash("beta"));
    }

    // A PNG that announces a 20000x20000 picture — 1.6 GB once it becomes RGBA
    // — behind a few hundred bytes of payload. The header is genuine, so
    // nothing before the decode would reject it; the allocation limit is what
    // turns this into a clean `None` instead of an allocation the process
    // cannot satisfy. Without the limit the decoder sizes its buffer from the
    // header alone.
    #[test]
    fn oversized_image_declarations_are_refused() {
        fn png_with_dimensions(width: u32, height: u32) -> Vec<u8> {
            let mut ihdr = vec![0u8; 13];
            ihdr[0..4].copy_from_slice(&width.to_be_bytes());
            ihdr[4..8].copy_from_slice(&height.to_be_bytes());
            ihdr[8] = 8; // bit depth
            ihdr[9] = 6; // colour type RGBA
            ihdr[10] = 0;
            ihdr[11] = 0;
            ihdr[12] = 0;

            fn crc32(data: &[u8]) -> u32 {
                let mut table = [0u32; 256];
                for (i, entry) in table.iter_mut().enumerate() {
                    let mut c = i as u32;
                    for _ in 0..8 {
                        c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                    }
                    *entry = c;
                }
                let mut crc = 0xFFFF_FFFFu32;
                for &b in data {
                    crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
                }
                crc ^ 0xFFFF_FFFF
            }

            fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
                out.extend_from_slice(&(body.len() as u32).to_be_bytes());
                let mut full = kind.to_vec();
                full.extend_from_slice(body);
                out.extend_from_slice(&full);
                out.extend_from_slice(&crc32(&full).to_be_bytes());
            }

            let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
            chunk(&mut png, b"IHDR", &ihdr);
            chunk(&mut png, b"IDAT", &[]);
            chunk(&mut png, b"IEND", &[]);
            png
        }

        use base64::Engine;
        let encode = |bytes: &[u8]| {
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        };

        // Well under the cap: 8K RGBA is ~132 MB.
        let eight_k = encode(&png_with_dimensions(8192, 4320));
        // Far over it: 20000x20000 RGBA would be 1.6 GB.
        let bomb = encode(&png_with_dimensions(20000, 20000));

        assert_eq!(
            calc_image_hash(&bomb),
            None,
            "a header that large must be refused rather than allocated"
        );
        // Both payloads are truncated, so both decode to None; what matters is
        // that the decision is made from the declared size, not from attempting
        // the allocation.
        let _ = calc_image_hash(&eight_k);
    }
}
