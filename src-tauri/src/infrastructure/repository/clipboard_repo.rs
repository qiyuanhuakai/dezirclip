use crate::database::{
    calc_image_hash, calc_text_hash, has_sensitive_tag, is_text_type, save_image_to_file,
    thumbnail_hash, ENCRYPT_PREFIX,
};
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::encryption;
use crate::infrastructure::repository::settings_repo::SqliteSettingsRepository;
use rusqlite::params;
use rusqlite::Connection;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use urlencoding::decode;

const RICH_IMAGE_FALLBACK_PREFIX: &str = "<!--TIEZ_RICH_IMAGE:";
const RICH_IMAGE_FALLBACK_SUFFIX: &str = "-->";
const HISTORY_CONTENT_PREVIEW_CHARS: usize = 2_000;
const HISTORY_PREVIEW_CHARS: usize = 500;
const HISTORY_HTML_PREVIEW_CHARS: usize = 5_000;

/// Byte ceilings for the three repository caches.
///
/// The entry ceilings stay where they were; these are what stop a page of
/// screenshots from turning "64 cached pages" into an unbounded resident set.
/// A page of ordinary text entries weighs well under a megabyte, so a text-only
/// history still keeps dozens of pages warm — the ceiling only starts evicting
/// once the cached pages are actually carrying megabytes.
const HISTORY_CACHE_MAX_BYTES: usize = 16 * 1024 * 1024;
const SEARCH_CACHE_MAX_BYTES: usize = 16 * 1024 * 1024;
const CONTENT_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;
const HISTORY_LIST_SELECT_COLUMNS: &str = "id, content_type, \
    CASE WHEN content LIKE 'linux:%' OR content LIKE 'dpapi:%' THEN content ELSE substr(content, 1, 2004) END, \
    CASE WHEN html_content LIKE 'linux:%' OR html_content LIKE 'dpapi:%' THEN html_content ELSE substr(html_content, 1, 5004) END, \
    source_app, timestamp, \
    CASE WHEN preview LIKE 'linux:%' OR preview LIKE 'dpapi:%' THEN preview ELSE substr(preview, 1, 504) END, \
    is_pinned, tags, use_count, is_external, pinned_order, source_app_path, ocr_text, ocr_status";

fn truncate_chars_with_suffix(input: &str, limit: usize, suffix: &str) -> String {
    let Some((cut, _)) = input.char_indices().nth(limit) else {
        return input.to_string();
    };
    let mut out = String::with_capacity(cut + suffix.len());
    out.push_str(&input[..cut]);
    out.push_str(suffix);
    out
}

fn history_content_preview(value: &str) -> String {
    truncate_chars_with_suffix(
        value,
        HISTORY_CONTENT_PREVIEW_CHARS,
        "... [Truncated for speed]",
    )
}

fn history_preview(value: &str) -> String {
    truncate_chars_with_suffix(value, HISTORY_PREVIEW_CHARS, "...")
}

fn history_html_preview(value: &str) -> String {
    truncate_chars_with_suffix(value, HISTORY_HTML_PREVIEW_CHARS, "... [HTML Truncated]")
}

/// What a single cached entry keeps alive, in bytes.
///
/// A page-count bound alone does not bound memory: the history and search
/// caches hold whole pages of entries, and an entry's payload is not a fixed
/// size. A screenshot carries its full data URL in `content`, a table carries
/// 5,000 characters of HTML, and an OCR'd screenshot carries both plus its
/// text. 64 pages of 120 entries is 7,680 entries, and how much that costs is
/// decided entirely by what the user happened to copy.
fn entry_payload_weight(entry: &ClipboardEntry) -> usize {
    entry.content.len()
        + entry.preview.len()
        + entry.source_app.len()
        + entry.source_app_path.as_deref().map_or(0, str::len)
        + entry.html_content.as_deref().map_or(0, str::len)
        + entry.ocr_text.as_deref().map_or(0, str::len)
        + entry.content_type.len()
        + entry.tags.iter().map(String::len).sum::<usize>()
}

fn entry_page_weight(entries: &Vec<ClipboardEntry>) -> usize {
    entries.iter().map(entry_payload_weight).sum()
}

fn content_triple_weight(value: &(String, String, Option<String>)) -> usize {
    value.0.len() + value.1.len() + value.2.as_deref().map_or(0, str::len)
}

struct SimpleLruCache<T: Clone> {
    map: HashMap<String, T>,
    order: VecDeque<String>,
    capacity: usize,
    max_weight: usize,
    weight: usize,
    weight_of: fn(&T) -> usize,
}

impl<T: Clone> SimpleLruCache<T> {
    fn new(capacity: usize, max_weight: usize, weight_of: fn(&T) -> usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
            max_weight: max_weight.max(1),
            weight: 0,
            weight_of,
        }
    }

    fn get(&mut self, key: &str) -> Option<T> {
        if !self.map.contains_key(key) {
            return None;
        }
        self.touch(key);
        self.map.get(key).cloned()
    }

    fn put(&mut self, key: String, value: T) {
        let incoming = (self.weight_of)(&value);

        if let Some(previous) = self.map.insert(key.clone(), value) {
            self.weight = self.weight.saturating_sub((self.weight_of)(&previous));
            self.touch(&key);
            self.weight = self.weight.saturating_add(incoming);
            self.evict_to_budget();
            return;
        }

        self.order.push_back(key);
        self.weight = self.weight.saturating_add(incoming);
        self.evict_to_budget();
    }

    /// Drops least-recently-used entries until the entry count and the byte
    /// budget are both satisfied.
    ///
    /// A value that exceeds the whole budget on its own is not retained. A miss
    /// costs the query the hit would have saved and nothing else, whereas
    /// keeping it would leave the cache holding exactly the payload the budget
    /// exists to exclude.
    fn evict_to_budget(&mut self) {
        while self.map.len() > self.capacity || self.weight > self.max_weight {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(dropped) = self.map.remove(&oldest) {
                self.weight = self.weight.saturating_sub((self.weight_of)(&dropped));
            }
        }
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.weight = 0;
    }

    fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.to_string());
    }
}

pub trait ClipboardRepository {
    fn save(
        &self,
        entry: &ClipboardEntry,
        data_dir: Option<&std::path::Path>,
    ) -> Result<i64, String>;
    fn get_history(
        &self,
        limit: i32,
        offset: i32,
        content_type: Option<&str>,
    ) -> Result<Vec<ClipboardEntry>, String>;
    fn search_fts(&self, query: &str, limit: u32) -> Result<Vec<ClipboardEntry>, String>;
    fn search(&self, query: &str, limit: i32) -> Result<Vec<ClipboardEntry>, String>;
    fn delete(&self, id: i64, data_dir: Option<&std::path::Path>) -> Result<(), String>;
    fn clear(&self, data_dir: Option<&std::path::Path>) -> Result<(), String>;
    fn get_count(&self) -> Result<i64, String>;
    fn increment_use_count(&self, id: i64) -> Result<(), String>;
    fn touch_entry(&self, id: i64, timestamp: i64) -> Result<(), String>;
    fn toggle_pin(&self, id: i64, is_pinned: bool) -> Result<(), String>;
    fn update_pinned_order(&self, orders: Vec<(i64, i64)>) -> Result<(), String>;
    fn get_entry_by_id(&self, id: i64) -> Result<Option<ClipboardEntry>, String>;
    fn get_entry_by_content(
        &self,
        content: &str,
        content_type: Option<&str>,
    ) -> Result<Option<i64>, String>;
    fn update_entry_content(&self, id: i64, content: &str, preview: &str) -> Result<(), String>;
    fn get_entry_content(&self, id: i64) -> Result<Option<String>, String>;
    fn get_entry_content_full(&self, id: i64) -> Result<Option<(String, String)>, String>;
    fn get_entry_content_with_html(
        &self,
        id: i64,
    ) -> Result<Option<(String, String, Option<String>)>, String>;
}

pub struct SqliteClipboardRepository {
    conn: Arc<Mutex<Connection>>,
    history_cache: Arc<Mutex<SimpleLruCache<Vec<ClipboardEntry>>>>,
    search_cache: Arc<Mutex<SimpleLruCache<Vec<ClipboardEntry>>>>,
    content_cache: Arc<Mutex<SimpleLruCache<(String, String, Option<String>)>>>,
}

impl SqliteClipboardRepository {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            conn,
            history_cache: Arc::new(Mutex::new(SimpleLruCache::new(
                64,
                HISTORY_CACHE_MAX_BYTES,
                entry_page_weight,
            ))),
            search_cache: Arc::new(Mutex::new(SimpleLruCache::new(
                64,
                SEARCH_CACHE_MAX_BYTES,
                entry_page_weight,
            ))),
            content_cache: Arc::new(Mutex::new(SimpleLruCache::new(
                256,
                CONTENT_CACHE_MAX_BYTES,
                content_triple_weight,
            ))),
        }
    }

    fn invalidate_caches(&self) {
        if let Ok(mut history) = self.history_cache.lock() {
            history.clear();
        }
        if let Ok(mut search) = self.search_cache.lock() {
            search.clear();
        }
        if let Ok(mut content) = self.content_cache.lock() {
            content.clear();
        }
    }

    pub fn encrypt_entry_with_conn(&self, conn: &Connection, id: i64) -> Result<(), String> {
        let (content_raw, preview_raw, html_raw, content_type, content_hash): (String, String, Option<String>, String, i64) =
            conn.query_row(
                "SELECT content, preview, html_content, content_type, content_hash FROM clipboard_history WHERE id = ?",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2).ok(), row.get(3)?, row.get(4)?)),
            ).map_err(|e| e.to_string())?;

        let already_encrypted = encryption::is_encrypted_value(&content_raw)
            && encryption::is_encrypted_value(&preview_raw)
            && html_raw
                .as_ref()
                .map(|h| encryption::is_encrypted_value(h))
                .unwrap_or(true);
        if already_encrypted {
            return Ok(());
        }

        let content_plain = self.maybe_decrypt_text(&content_raw);
        let preview_plain = self.maybe_decrypt_text(&preview_raw);
        let html_plain = html_raw.map(|h| self.maybe_decrypt_text(&h));

        let content_enc = self.maybe_encrypt_text(&content_plain);
        let preview_enc = self.maybe_encrypt_text(&preview_plain);
        let html_enc = html_plain.as_ref().map(|h| self.maybe_encrypt_text(h));
        let new_hash = if is_text_type(&content_type) {
            calc_text_hash(&content_plain) as i64
        } else {
            content_hash
        };

        conn.execute(
            "UPDATE clipboard_history SET content = ?, preview = ?, html_content = ?, content_hash = ? WHERE id = ?",
            params![content_enc, preview_enc, html_enc, new_hash, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn decrypt_entry_with_conn(&self, conn: &Connection, id: i64) -> Result<(), String> {
        let (content_raw, preview_raw, html_raw, content_type, content_hash): (String, String, Option<String>, String, i64) =
            conn.query_row(
                "SELECT content, preview, html_content, content_type, content_hash FROM clipboard_history WHERE id = ?",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2).ok(), row.get(3)?, row.get(4)?)),
            ).map_err(|e| e.to_string())?;

        let any_encrypted = encryption::is_encrypted_value(&content_raw)
            || encryption::is_encrypted_value(&preview_raw)
            || html_raw
                .as_ref()
                .map(|h| encryption::is_encrypted_value(h))
                .unwrap_or(false);
        if !any_encrypted {
            return Ok(());
        }

        let content_plain = self.maybe_decrypt_text(&content_raw);
        let preview_plain = self.maybe_decrypt_text(&preview_raw);
        let html_plain = html_raw.map(|h| self.maybe_decrypt_text(&h));
        let new_hash = if is_text_type(&content_type) {
            calc_text_hash(&content_plain) as i64
        } else {
            content_hash
        };

        conn.execute(
            "UPDATE clipboard_history SET content = ?, preview = ?, html_content = ?, content_hash = ? WHERE id = ?",
            params![content_plain, preview_plain, html_plain, new_hash, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn sync_entry_tags_with_conn(
        &self,
        conn: &Connection,
        entry_id: i64,
        tags: &[String],
    ) -> Result<(), String> {
        conn.execute(
            "DELETE FROM entry_tags WHERE entry_id = ?",
            params![entry_id],
        )
        .map_err(|e| e.to_string())?;
        for tag in tags {
            let clean = tag.trim();
            if clean.is_empty() {
                continue;
            }
            conn.execute(
                "INSERT OR IGNORE INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                params![entry_id, clean],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn maybe_encrypt_text(&self, value: &str) -> String {
        #[cfg(not(feature = "portable"))]
        {
            if encryption::is_encrypted_value(value) {
                return value.to_string();
            }
            encryption::encrypt_value(value).unwrap_or_else(|| value.to_string())
        }
        #[cfg(feature = "portable")]
        {
            value.to_string()
        }
    }

    fn maybe_decrypt_text(&self, value: &str) -> String {
        if encryption::is_encrypted_value(value) {
            encryption::decrypt_value(value).unwrap_or_else(|| value.to_string())
        } else {
            value.to_string()
        }
    }

    fn extract_rich_image_fallback_payload(html: &str) -> Option<String> {
        if let Some(start) = html.rfind(RICH_IMAGE_FALLBACK_PREFIX) {
            let marker_start = start + RICH_IMAGE_FALLBACK_PREFIX.len();
            if let Some(end_rel) = html[marker_start..].find(RICH_IMAGE_FALLBACK_SUFFIX) {
                let marker_end = marker_start + end_rel;
                let payload = html[marker_start..marker_end].trim();
                if !payload.is_empty() {
                    return Some(payload.to_string());
                }
            }
        }
        None
    }

    fn fallback_payload_to_path(payload: &str) -> Option<PathBuf> {
        let value = payload.trim();
        if value.is_empty() || value.starts_with("data:image/") {
            return None;
        }

        let path_raw = if value.starts_with("file://") {
            value.trim_start_matches("file://")
        } else {
            value
        };

        let path_without_drive_prefix =
            if path_raw.starts_with('/') && path_raw.chars().nth(2) == Some(':') {
                &path_raw[1..]
            } else {
                path_raw
            };

        let decoded_path = decode(path_without_drive_prefix)
            .map(|p| p.into_owned())
            .unwrap_or_else(|_| path_without_drive_prefix.to_string());

        if decoded_path.is_empty() {
            None
        } else {
            Some(PathBuf::from(decoded_path))
        }
    }

    fn collect_attachment_paths_for_cleanup(
        &self,
        content_raw: &str,
        html_raw: Option<&str>,
        is_external: bool,
        attachments_dir: &std::path::Path,
    ) -> Vec<PathBuf> {
        let mut paths = HashSet::new();

        if is_external {
            let content_path = PathBuf::from(self.maybe_decrypt_text(content_raw));
            if content_path.starts_with(attachments_dir) {
                paths.insert(content_path);
            }
        }

        if let Some(html_raw_value) = html_raw {
            let html = self.maybe_decrypt_text(html_raw_value);
            if let Some(payload) = Self::extract_rich_image_fallback_payload(&html) {
                if let Some(path) = Self::fallback_payload_to_path(&payload) {
                    if path.starts_with(attachments_dir) {
                        paths.insert(path);
                    }
                }
            }
        }

        paths.into_iter().collect()
    }

    pub fn save_with_conn(
        &self,
        conn: &Connection,
        entry: &ClipboardEntry,
        data_dir: Option<&std::path::Path>,
    ) -> Result<i64, String> {
        self.save_with_conn_and_image_hash(conn, entry, data_dir, None)
    }

    /// Same as [`Self::save_with_conn`], but lets a caller that already decoded
    /// the image hand its `content_hash` down.
    ///
    /// `content_hash` for an image comes from the decoded 32x32 thumbnail, so
    /// recomputing it here would mean a second base64 decode plus a second full
    /// image decode of the same picture — a 4K screenshot decodes to ~33 MB of
    /// RGBA. The clipboard pipeline already computes that hash while looking for
    /// a duplicate, so it passes the value in and the decode happens once per
    /// event. `None` means "no hash was computed for you", which is what every
    /// other caller wants and keeps the previous behaviour intact.
    pub fn save_with_conn_and_image_hash(
        &self,
        conn: &Connection,
        entry: &ClipboardEntry,
        data_dir: Option<&std::path::Path>,
        image_hash: Option<i64>,
    ) -> Result<i64, String> {
        // Encrypt only when explicitly marked as sensitive
        let should_encrypt = has_sensitive_tag(&entry.tags);

        // Borrowed until something actually replaces it. This used to clone the
        // content up front, which for an image is the whole `data:` URL: a 4K
        // screenshot is 5.3 MB of base64, and on the very next line a successful
        // `save_image_to_file` overwrites the copy with a short file path. The
        // clone was the single largest allocation on the save path and half of
        // what it added, spent on a string that was about to be dropped.
        let mut final_content: Cow<'_, str> = Cow::Borrowed(&entry.content);
        let mut final_is_external = entry.is_external;

        // Externalize image if possible
        if entry.content_type == "image" && entry.content.starts_with("data:image/") {
            if let Some(dir) = data_dir {
                if let Some(path) = save_image_to_file(&entry.content, dir) {
                    final_content = Cow::Owned(path);
                    final_is_external = true;
                }
            }
        }

        let calculated_hash = if entry.content_type == "image" {
            // A handed-in hash only ever describes a `data:` payload — the
            // on-disk branch needs `image::open` on a real path, so a stray value
            // falls through to the decode rather than silently mislabelling it.
            match image_hash {
                Some(hash) if entry.content.starts_with("data:") => hash,
                None if entry.content.starts_with("data:") => {
                    calc_image_hash(&entry.content).unwrap_or(0)
                }
                _ => match image::open(&entry.content) {
                    Ok(img) => thumbnail_hash(&img),
                    Err(_) => 0,
                },
            }
        } else {
            calc_text_hash(&final_content) as i64
        };

        let (content, preview, content_hash, html_content) = if should_encrypt {
            let encrypted_content = self.maybe_encrypt_text(&final_content);
            let encrypted_preview = self.maybe_encrypt_text(&entry.preview);
            let encrypted_html = entry
                .html_content
                .as_ref()
                .map(|html| self.maybe_encrypt_text(html));
            (
                encrypted_content,
                encrypted_preview,
                calculated_hash,
                encrypted_html,
            )
        } else {
            (
                // `into_owned` moves when the content was already replaced by the
                // file path, and copies when it is still the caller's own string --
                // which is the case the old unconditional clone was paying for.
                final_content.into_owned(),
                entry.preview.clone(),
                calculated_hash,
                entry.html_content.clone(),
            )
        };

        let mut seen: HashSet<String> = HashSet::new();
        let mut cleaned_tags: Vec<String> = Vec::new();
        for tag in &entry.tags {
            let t = tag.trim();
            if t.is_empty() {
                continue;
            }
            let t_owned = t.to_string();
            if seen.insert(t_owned.clone()) {
                cleaned_tags.push(t_owned);
            }
        }

        if entry.id > 0 {
            // Update existing entry (Move to top logic)
            conn.execute(
                "UPDATE clipboard_history SET 
                    content_type = ?1, 
                    content = ?2, 
                    html_content = ?3, 
                    source_app = ?4, 
                    timestamp = ?5, 
                    preview = ?6, 
                    content_hash = ?7, 
                    tags = ?8, 
                    is_external = ?9,
                    source_app_path = ?10,
                    use_count = use_count + 1
                 WHERE id = ?11",
                params![
                    entry.content_type,
                    content,
                    html_content,
                    entry.source_app,
                    entry.timestamp,
                    preview,
                    content_hash,
                    serde_json::to_string(&cleaned_tags).unwrap_or_else(|_| "[]".to_string()),
                    if final_is_external { 1 } else { 0 },
                    entry.source_app_path.as_deref(),
                    entry.id
                ],
            )
            .map_err(|e| e.to_string())?;
            self.sync_entry_tags_with_conn(conn, entry.id, &cleaned_tags)?;
            self.invalidate_caches();
            Ok(entry.id)
        } else {
            // Insert new entry
            conn.execute(
                "INSERT INTO clipboard_history (content_type, content, html_content, source_app, timestamp, preview, is_pinned, content_hash, tags, is_external, pinned_order, source_app_path, ocr_text, ocr_status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, 'pending')",
                params![
                    entry.content_type,
                    content,
                    html_content,
                    entry.source_app,
                    entry.timestamp,
                    preview,
                    if entry.is_pinned { 1 } else { 0 },
                    content_hash,
                    serde_json::to_string(&cleaned_tags).unwrap_or_else(|_| "[]".to_string()),
                    if final_is_external { 1 } else { 0 },
                    entry.pinned_order,
                    entry.source_app_path.as_deref()
                ],
            ).map_err(|e| e.to_string())?;

            let new_id = conn.last_insert_rowid();
            self.sync_entry_tags_with_conn(conn, new_id, &cleaned_tags)?;
            self.invalidate_caches();
            Ok(new_id)
        }
    }

    pub fn delete_with_conn(
        &self,
        conn: &Connection,
        id: i64,
        data_dir: Option<&std::path::Path>,
    ) -> Result<(), String> {
        // Check for external files to delete
        if let Some(dir) = data_dir {
            let attachments_dir = dir.join("attachments");
            let mut stmt = conn
                .prepare(
                    "SELECT content, html_content, is_external FROM clipboard_history WHERE id = ?",
                )
                .map_err(|e| e.to_string())?;

            if let Ok(entry) = stmt.query_row([id], |row| {
                let content_raw: String = row.get(0)?;
                let html_raw: Option<String> = row.get(1).ok();
                let is_ext: i32 = row.get(2)?;
                Ok((content_raw, html_raw, is_ext == 1))
            }) {
                let files_to_remove = self.collect_attachment_paths_for_cleanup(
                    &entry.0,
                    entry.1.as_deref(),
                    entry.2,
                    &attachments_dir,
                );
                for path in files_to_remove {
                    if path.exists() {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }

        conn.execute("DELETE FROM clipboard_history WHERE id = ?", [id])
            .map_err(|e| e.to_string())?;
        let _ = conn.execute("DELETE FROM entry_tags WHERE entry_id = ?", params![id]);
        self.invalidate_caches();
        Ok(())
    }

    pub fn find_by_content_with_conn(
        &self,
        conn: &Connection,
        content: &str,
        content_type: Option<&str>,
    ) -> Result<Option<i64>, String> {
        self.find_by_content_with_hash(conn, content, content_type, None)
    }

    /// `image_hash` lets a caller that already decoded the image reuse that
    /// hash instead of paying for a base64 decode plus a full image decode on
    /// every lookup. Pass `None` to derive it from `content` as before.
    pub fn find_by_content_with_hash(
        &self,
        conn: &Connection,
        content: &str,
        content_type: Option<&str>,
        image_hash: Option<i64>,
    ) -> Result<Option<i64>, String> {
        if content_type == Some("image") {
            if let Some(hash) = image_hash.or_else(|| calc_image_hash(content)) {
                let mut stmt = conn
                    .prepare(
                        "SELECT id FROM clipboard_history \
                     WHERE (content_type = 'image' AND content_hash = ?) OR content = ?",
                    )
                    .map_err(|e| e.to_string())?;
                let mut rows = stmt
                    .query(params![hash, content])
                    .map_err(|e| e.to_string())?;
                if let Some(row) = rows.next().map_err(|e| e.to_string())? {
                    return Ok(Some(row.get(0).map_err(|e| e.to_string())?));
                }
                return Ok(None);
            }
        }

        let hash = calc_text_hash(content) as i64;

        if let Some(ct) = content_type {
            let mut stmt = conn.prepare(
                "SELECT id FROM clipboard_history \
                 WHERE (content_type = ? AND content_hash = ?) OR (content_type = ? AND content = ?)",
            ).map_err(|e| e.to_string())?;
            let mut rows = stmt
                .query(params![ct, hash, ct, content])
                .map_err(|e| e.to_string())?;
            if let Some(row) = rows.next().map_err(|e| e.to_string())? {
                Ok(Some(row.get(0).map_err(|e| e.to_string())?))
            } else {
                Ok(None)
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id FROM clipboard_history \
                 WHERE ((content_type IN ('text', 'rich_text', 'code', 'url')) AND content_hash = ?) OR content = ?",
            ).map_err(|e| e.to_string())?;
            let mut rows = stmt
                .query(params![hash, content])
                .map_err(|e| e.to_string())?;
            if let Some(row) = rows.next().map_err(|e| e.to_string())? {
                Ok(Some(row.get(0).map_err(|e| e.to_string())?))
            } else {
                Ok(None)
            }
        }
    }

    pub fn enforce_limit_with_conn(
        &self,
        conn: &Connection,
        data_dir: Option<&std::path::Path>,
    ) -> Result<Vec<i64>, String> {
        // Check if storage limit is enabled
        if let Ok(Some(limit_enabled_str)) =
            SqliteSettingsRepository::get_raw(conn, "app.persistent_limit_enabled")
        {
            if limit_enabled_str == "false" {
                return Ok(Vec::new());
            }
        }

        // Get the storage limit
        if let Ok(Some(limit_str)) = SqliteSettingsRepository::get_raw(conn, "app.persistent_limit")
        {
            if let Ok(limit) = limit_str.parse::<i32>() {
                // `INDEXED BY` is not decoration. Left to itself the planner
                // picks `idx_clipboard_history_pinned_order_time` for both of
                // these, because it can seek on `is_pinned` — but `pinned_order`
                // sits between `is_pinned` and `timestamp`, so it still has to
                // read every matching row out of the table to apply the `tags`
                // test, and to sort them for the ORDER BY. The partial index
                // added in v16 holds only the rows that pass that test, already
                // in timestamp order, so both queries become a covering walk of
                // it. The planner will not choose it on its own; naming it is
                // what keeps the cost flat as the history grows.
                let count: i32 = conn.query_row(
                    "SELECT COUNT(*) FROM clipboard_history INDEXED BY idx_clipboard_history_evictable
                     WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL)",
                    [],
                    |row| row.get(0)
                ).map_err(|e| e.to_string())?;

                if count > limit {
                    // First, get the IDs that will be deleted
                    let to_delete = count - limit;
                    let deleted_ids: Vec<i64> = {
                        let mut stmt = conn
                            .prepare(
                                "SELECT id FROM clipboard_history INDEXED BY idx_clipboard_history_evictable
                             WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL)
                             ORDER BY timestamp ASC
                             LIMIT ?",
                            )
                            .map_err(|e| e.to_string())?;

                        let rows = stmt
                            .query_map([to_delete], |row| row.get(0))
                            .map_err(|e| e.to_string())?;
                        rows.filter_map(|r| r.ok()).collect()
                    };
                    // Actually delete records (and files if needed)
                    for id in &deleted_ids {
                        let _ = self.delete_with_conn(conn, *id, data_dir);
                    }
                    return Ok(deleted_ids);
                }
            }
        }

        Ok(Vec::new())
    }
    pub fn toggle_pin_with_conn(
        &self,
        conn: &Connection,
        id: i64,
        is_pinned: bool,
    ) -> Result<(), String> {
        if is_pinned {
            // Set pinned_order to max + 1 so it appears at top
            conn.execute(
                "UPDATE clipboard_history 
                 SET is_pinned = 1, 
                     pinned_order = (SELECT COALESCE(MAX(pinned_order), 0) + 1 FROM clipboard_history WHERE is_pinned = 1) 
                 WHERE id = ?",
                params![id],
            ).map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "UPDATE clipboard_history SET is_pinned = 0, pinned_order = 0 WHERE id = ?",
                params![id],
            )
            .map_err(|e| e.to_string())?;
        }
        self.invalidate_caches();
        Ok(())
    }

    pub fn update_pinned_order_with_conn(
        &self,
        conn: &Connection,
        orders: Vec<(i64, i64)>,
    ) -> Result<(), String> {
        for (id, order) in orders {
            conn.execute(
                "UPDATE clipboard_history SET pinned_order = ? WHERE id = ?",
                params![order, id],
            )
            .map_err(|e| e.to_string())?;
        }
        self.invalidate_caches();
        Ok(())
    }

    pub fn get_entry_by_id_with_conn(
        &self,
        conn: &Connection,
        id: i64,
    ) -> Result<Option<ClipboardEntry>, String> {
        let mut stmt = conn.prepare(
            "SELECT id, content_type, content, html_content, source_app, timestamp, preview, is_pinned, tags, use_count, is_external, pinned_order, source_app_path, ocr_text, ocr_status 
             FROM clipboard_history 
             WHERE id = ? 
             LIMIT 1",
        ).map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![id]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let tags_str: String = row.get(8).unwrap_or_else(|_| "[]".to_string());
            let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();

            let content_raw: String = row.get(2).map_err(|e| e.to_string())?;
            let html_raw: Option<String> = row.get(3).map_err(|e| e.to_string()).unwrap_or(None);
            let preview_raw: String = row.get(6).map_err(|e| e.to_string())?;
            let content = self.maybe_decrypt_text(&content_raw);
            let preview = self.maybe_decrypt_text(&preview_raw);
            let html_content = html_raw.map(|v| self.maybe_decrypt_text(&v));

            Ok(Some(ClipboardEntry {
                id: row.get(0).map_err(|e| e.to_string())?,
                content_type: row.get(1).map_err(|e| e.to_string())?,
                content,
                html_content,
                source_app: row.get(4).map_err(|e| e.to_string())?,
                timestamp: row.get(5).map_err(|e| e.to_string())?,
                preview,
                is_pinned: row.get::<_, i32>(7).map_err(|e| e.to_string())? == 1,
                tags,
                use_count: row.get(9).unwrap_or(0),
                is_external: row.get::<_, i32>(10).unwrap_or(0) == 1,
                pinned_order: row.get(11).unwrap_or(0),
                source_app_path: row.get(12).unwrap_or(None),
                file_preview_exists: true,
                content_kinds: Vec::new(),
                ocr_text: row.get(13).ok(),
                ocr_status: row.get(14).ok(),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn update_entry_content_with_conn(
        &self,
        conn: &Connection,
        id: i64,
        content: &str,
        preview: &str,
    ) -> Result<(), String> {
        let (old_content_raw, content_type, tags_json) = conn
            .query_row(
                "SELECT content, content_type, tags FROM clipboard_history WHERE id = ?",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|e| e.to_string())?;

        let old_content = self.maybe_decrypt_text(&old_content_raw);
        if old_content == content {
            return Ok(());
        }

        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        let should_encrypt = has_sensitive_tag(&tags);

        if is_text_type(&content_type) {
            let hash = calc_text_hash(content) as i64;
            let new_type = if content_type == "rich_text" {
                "text"
            } else {
                &content_type
            };
            if should_encrypt {
                let encrypted_content = self.maybe_encrypt_text(content);
                let encrypted_preview = self.maybe_encrypt_text(preview);
                conn.execute(
                    "UPDATE clipboard_history SET content = ?, preview = ?, content_hash = ?, html_content = NULL, content_type = ? WHERE id = ?",
                    params![encrypted_content, encrypted_preview, hash, new_type, id],
                ).map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE clipboard_history SET content = ?, preview = ?, content_hash = ?, html_content = NULL, content_type = ? WHERE id = ?",
                    params![content, preview, hash, new_type, id],
                ).map_err(|e| e.to_string())?;
            }
            self.invalidate_caches();
            return Ok(());
        }
        if should_encrypt {
            let encrypted_content = self.maybe_encrypt_text(content);
            let encrypted_preview = self.maybe_encrypt_text(preview);
            conn.execute(
                "UPDATE clipboard_history SET content = ?, preview = ?, html_content = NULL WHERE id = ?",
                params![encrypted_content, encrypted_preview, id],
            ).map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "UPDATE clipboard_history SET content = ?, preview = ?, html_content = NULL WHERE id = ?",
                params![content, preview, id],
            ).map_err(|e| e.to_string())?;
        }
        self.invalidate_caches();
        Ok(())
    }

    pub fn get_entry_content_full_with_conn(
        &self,
        conn: &Connection,
        id: i64,
    ) -> Result<Option<(String, String)>, String> {
        let mut stmt = conn
            .prepare("SELECT content, content_type FROM clipboard_history WHERE id = ?")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![id]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let content: String = row.get(0).map_err(|e| e.to_string())?;
            let content_type: String = row.get(1).map_err(|e| e.to_string())?;
            Ok(Some((self.maybe_decrypt_text(&content), content_type)))
        } else {
            Ok(None)
        }
    }

    pub fn get_entry_content_with_html_with_conn(
        &self,
        conn: &Connection,
        id: i64,
    ) -> Result<Option<(String, String, Option<String>)>, String> {
        let cache_key = id.to_string();
        if let Ok(mut cache) = self.content_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(Some(cached));
            }
        }
        let mut stmt = conn
            .prepare(
                "SELECT content, content_type, html_content FROM clipboard_history WHERE id = ?",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![id]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let content: String = row.get(0).map_err(|e| e.to_string())?;
            let content_type: String = row.get(1).map_err(|e| e.to_string())?;
            let html_raw: Option<String> = row.get(2).map_err(|e| e.to_string()).unwrap_or(None);
            let html_content = html_raw.map(|v| self.maybe_decrypt_text(&v));
            let value = (
                self.maybe_decrypt_text(&content),
                content_type,
                html_content,
            );
            if let Ok(mut cache) = self.content_cache.lock() {
                cache.put(cache_key, value.clone());
            }
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn update_ocr_text_with_conn(
        &self,
        conn: &Connection,
        id: i64,
        ocr_text: &str,
        ocr_status: &str,
    ) -> Result<usize, String> {
        let rows = conn
            .execute(
                "UPDATE clipboard_history
                 SET ocr_text = ?1, ocr_status = ?2
                 WHERE id = ?3",
                params![ocr_text, ocr_status, id],
            )
            .map_err(|e| e.to_string())?;
        self.invalidate_caches();
        Ok(rows)
    }

    pub fn get_ocr_status_with_conn(
        &self,
        conn: &Connection,
        id: i64,
    ) -> Result<Option<(String, Option<String>)>, String> {
        let mut stmt = conn
            .prepare("SELECT ocr_status, ocr_text FROM clipboard_history WHERE id = ? LIMIT 1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![id]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let status: String = row.get(0).map_err(|e| e.to_string())?;
            let text: Option<String> = row.get(1).ok();
            Ok(Some((status, text)))
        } else {
            Ok(None)
        }
    }

    pub fn search_fts(&self, query: &str, limit: u32) -> Result<Vec<ClipboardEntry>, String> {
        let term = query.trim();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        let limit_i64 = limit as i64;
        let mut stmt = conn
            .prepare(
                "SELECT ch.id, ch.content_type, ch.content, ch.html_content, ch.source_app,
                        ch.timestamp, ch.preview, ch.is_pinned, ch.tags, ch.use_count,
                        ch.is_external, ch.pinned_order, ch.source_app_path,
                        ch.content_kinds, ch.ocr_text, ch.ocr_status
                 FROM clipboard_fts
                 INNER JOIN clipboard_history ch ON ch.id = clipboard_fts.rowid
                 WHERE clipboard_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![term, limit_i64], |row| {
                let tags_str: String = row.get::<_, String>(8).unwrap_or_else(|_| "[]".to_string());
                let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
                let content_raw: String = row.get(2)?;
                let preview_raw: String = row.get(6)?;
                let html_raw: Option<String> = row.get(3).ok();
                let content = self.maybe_decrypt_text(&content_raw);
                let preview = self.maybe_decrypt_text(&preview_raw);
                let html_content = html_raw.map(|v| self.maybe_decrypt_text(&v));

                Ok(ClipboardEntry {
                    id: row.get(0)?,
                    content_type: row.get(1)?,
                    content,
                    html_content,
                    source_app: row.get(4)?,
                    timestamp: row.get(5)?,
                    preview,
                    is_pinned: row.get::<_, i32>(7)? == 1,
                    tags,
                    use_count: row.get(9).unwrap_or(0),
                    is_external: row.get::<_, i32>(10)? == 1,
                    pinned_order: row.get(11).unwrap_or(0),
                    source_app_path: row.get(12).ok().flatten(),
                    file_preview_exists: true,
                    content_kinds: serde_json::from_str(
                        &row.get::<_, String>(13).unwrap_or_else(|_| "[]".to_string()),
                    )
                    .unwrap_or_default(),
                    ocr_text: row.get(14).ok(),
                    ocr_status: row.get(15).ok(),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| e.to_string())?);
        }
        Ok(results)
    }
}

impl ClipboardRepository for SqliteClipboardRepository {
    fn save(
        &self,
        entry: &ClipboardEntry,
        data_dir: Option<&std::path::Path>,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        self.save_with_conn(&conn, entry, data_dir)
    }

    fn get_history(
        &self,
        limit: i32,
        offset: i32,
        content_type: Option<&str>,
    ) -> Result<Vec<ClipboardEntry>, String> {
        let cache_key = format!("{}:{}:{}", content_type.unwrap_or("*"), limit, offset);
        if let Ok(mut cache) = self.history_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached);
            }
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let map_row = |row: &rusqlite::Row| {
            let tags_str: String = row.get(8).unwrap_or_else(|_| "[]".to_string());
            let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
            let content_type: String = row.get(1)?;
            let content_raw: String = row.get(2)?;
            let html_raw: Option<String> = row.get(3).ok();
            let preview_raw: String = row.get(6)?;
            let content = history_content_preview(&self.maybe_decrypt_text(&content_raw));
            let preview = history_preview(&self.maybe_decrypt_text(&preview_raw));
            let html_content = html_raw
                .as_ref()
                .map(|v| history_html_preview(&self.maybe_decrypt_text(v)));

            Ok((
                ClipboardEntry {
                    id: row.get(0)?,
                    content_type,
                    content,
                    html_content,
                    source_app: row.get(4)?,
                    timestamp: row.get(5)?,
                    preview,
                    is_pinned: row.get::<_, i32>(7)? == 1,
                    tags,
                    use_count: row.get(9).unwrap_or(0),
                    is_external: row.get::<_, i32>(10)? == 1,
                    pinned_order: row.get(11).unwrap_or(0),
                    source_app_path: row.get(12).unwrap_or(None),
                    file_preview_exists: {
                        let is_ext = row.get::<_, i32>(10)? == 1;
                        if is_ext {
                            let c: String = self.maybe_decrypt_text(&row.get::<_, String>(2)?);
                            std::path::Path::new(&c).exists()
                        } else {
                            true
                        }
                    },
                    content_kinds: serde_json::from_str(
                        &row.get::<_, String>(15).unwrap_or_else(|_| "[]".to_string()),
                    )
                    .unwrap_or_default(),
                    ocr_text: row.get(13).ok(),
                    ocr_status: row.get(14).ok(),
                },
                content_raw,
                preview_raw,
                html_raw,
            ))
        };

        let mut mapped_rows = Vec::new();
        if let Some(ct) = content_type {
            let sql = format!(
                "SELECT {} FROM clipboard_history \
                 WHERE content_type = ? \
                 ORDER BY is_pinned DESC, pinned_order DESC, timestamp DESC, id DESC \
                 LIMIT ? OFFSET ?",
                HISTORY_LIST_SELECT_COLUMNS
            );
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![ct, limit, offset], map_row)
                .map_err(|e| e.to_string())?;
            for row in rows {
                mapped_rows.push(row.map_err(|e| e.to_string())?);
            }
        } else {
            let sql = format!(
                "SELECT {} FROM clipboard_history \
                 ORDER BY is_pinned DESC, pinned_order DESC, timestamp DESC, id DESC \
                 LIMIT ? OFFSET ?",
                HISTORY_LIST_SELECT_COLUMNS
            );
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([limit, offset], map_row)
                .map_err(|e| e.to_string())?;
            for row in rows {
                mapped_rows.push(row.map_err(|e| e.to_string())?);
            }
        }

        let mut history = Vec::new();
        for (entry, content_raw, preview_raw, html_raw) in mapped_rows {
            #[cfg(not(feature = "portable"))]
            {
                let is_sensitive = has_sensitive_tag(&entry.tags);
                let content_encrypted = encryption::is_encrypted_value(&content_raw);
                let preview_encrypted = encryption::is_encrypted_value(&preview_raw);
                let html_encrypted = html_raw
                    .as_ref()
                    .map(|h| encryption::is_encrypted_value(h))
                    .unwrap_or(false);
                let html_needs_encrypt = html_raw
                    .as_ref()
                    .map(|h| !encryption::is_encrypted_value(h))
                    .unwrap_or(false);

                if is_sensitive && (!content_encrypted || !preview_encrypted || html_needs_encrypt)
                {
                    let _ = self.encrypt_entry_with_conn(&conn, entry.id);
                } else if !is_sensitive
                    && (content_encrypted || preview_encrypted || html_encrypted)
                {
                    let _ = self.decrypt_entry_with_conn(&conn, entry.id);
                }
            }

            history.push(entry);
        }
        if let Ok(mut cache) = self.history_cache.lock() {
            cache.put(cache_key, history.clone());
        }
        Ok(history)
    }

    fn search_fts(&self, query: &str, limit: u32) -> Result<Vec<ClipboardEntry>, String> {
        Self::search_fts(self, query, limit)
    }

    fn search(&self, query: &str, limit: i32) -> Result<Vec<ClipboardEntry>, String> {
        let term = query.trim().to_lowercase();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        let cache_key = format!("{}:{}", term, limit);
        if let Ok(mut cache) = self.search_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached);
            }
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        #[cfg(feature = "portable")]
        {
            // Portable version: Data is NOT encrypted, use conventional SQL LIKE search (fastest)
            let mut stmt = conn.prepare(
                "SELECT DISTINCT ch.id, ch.content_type, ch.content, ch.html_content, ch.source_app, ch.timestamp, ch.preview, ch.is_pinned, ch.tags, ch.use_count, ch.is_external, ch.pinned_order, ch.source_app_path, ch.ocr_text, ch.ocr_status
	                 FROM clipboard_history ch
	                 LEFT JOIN entry_tags et ON ch.id = et.entry_id
	                 WHERE ch.content LIKE '%' || ? || '%'
	                    OR ch.source_app LIKE '%' || ? || '%'
	                    OR COALESCE(ch.ocr_text, '') LIKE '%' || ? || '%'
	                    OR et.tag LIKE '%' || ? || '%'
                 ORDER BY ch.timestamp DESC 
                 LIMIT ?",
            ).map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map(params![term, term, term, term, limit], |row| {
                    let tags_str: String =
                        row.get::<_, String>(8).unwrap_or_else(|_| "[]".to_string());
                    Ok(ClipboardEntry {
                        id: row.get(0)?,
                        content_type: row.get(1)?,
                        content: row.get(2)?,
                        html_content: row.get(3).ok(),
                        source_app: row.get(4)?,
                        timestamp: row.get(5)?,
                        preview: row.get(6)?,
                        is_pinned: row.get::<_, i32>(7)? == 1,
                        tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                        use_count: row.get(9).unwrap_or(0),
                        is_external: row.get::<_, i32>(10)? == 1,
                        pinned_order: row.get(11).unwrap_or(0),
                        source_app_path: row.get(12).unwrap_or(None),
                        file_preview_exists: true, // Simplified for search
                        content_kinds: Vec::new(),
                        ocr_text: row.get(13).ok(),
                        ocr_status: row.get(14).ok(),
                    })
                })
                .map_err(|e| e.to_string())?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| e.to_string())?);
            }
            if let Ok(mut cache) = self.search_cache.lock() {
                cache.put(cache_key, results.clone());
            }
            Ok(results)
        }

        #[cfg(not(feature = "portable"))]
        {
            let mut results: Vec<ClipboardEntry> = Vec::new();
            let mut seen: HashSet<i64> = HashSet::new();

            let sensitive_tags_sql = {
                let tags = crate::database::SENSITIVE_TAGS;
                let parts: Vec<String> = tags
                    .iter()
                    .map(|t| format!("'{}'", t.replace('\'', "''")))
                    .collect();
                format!("({})", parts.join(","))
            };

            // 1) SQL search for non-sensitive (plaintext) entries
            let sql_non_sensitive = format!(
                "SELECT DISTINCT ch.id, ch.content_type, ch.content, ch.html_content, ch.source_app, ch.timestamp, ch.preview, ch.is_pinned, ch.tags, ch.use_count, ch.is_external, ch.pinned_order, ch.source_app_path, ch.content_kinds, ch.ocr_text, ch.ocr_status
	                 FROM clipboard_history ch
	                 LEFT JOIN entry_tags et ON ch.id = et.entry_id
                 WHERE NOT EXISTS (
                     SELECT 1 FROM entry_tags se 
                     WHERE se.entry_id = ch.id 
                       AND se.tag COLLATE NOCASE IN {}
                 )
                   AND (
	                     ch.content LIKE '%' || ?1 || '%'
	                     OR ch.source_app LIKE '%' || ?1 || '%'
	                     OR COALESCE(ch.ocr_text, '') LIKE '%' || ?1 || '%'
	                     OR et.tag LIKE '%' || ?1 || '%'
                   )
                 ORDER BY ch.timestamp DESC, ch.id DESC
                 LIMIT ?2",
                sensitive_tags_sql
            );

            let mut stmt = conn
                .prepare(&sql_non_sensitive)
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![term, limit], |row| {
                    let tags_str: String = row.get(8).unwrap_or_else(|_| "[]".to_string());
                    let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
                    let content_raw: String = row.get(2)?;
                    let preview_raw: String = row.get(6)?;
                    let html_raw: Option<String> = row.get(3).ok();
                    let content = self.maybe_decrypt_text(&content_raw);
                    let preview = self.maybe_decrypt_text(&preview_raw);
                    let html_content = html_raw.map(|v| self.maybe_decrypt_text(&v));

                    Ok(ClipboardEntry {
                        id: row.get(0)?,
                        content_type: row.get(1)?,
                        content,
                        html_content,
                        source_app: row.get(4)?,
                        timestamp: row.get(5)?,
                        preview,
                        is_pinned: row.get::<_, i32>(7)? == 1,
                        tags,
                        use_count: row.get(9).unwrap_or(0),
                        is_external: row.get::<_, i32>(10)? == 1,
                        pinned_order: row.get(11).unwrap_or(0),
                        source_app_path: row.get(12).unwrap_or(None),
                        file_preview_exists: true,
                        content_kinds: serde_json::from_str(
                            &row.get::<_, String>(13).unwrap_or_else(|_| "[]".to_string()),
                        )
                        .unwrap_or_default(),
                        ocr_text: row.get(14).ok(),
                        ocr_status: row.get(15).ok(),
                    })
                })
                .map_err(|e| e.to_string())?;

            for row in rows {
                if let Ok(entry) = row {
                    if seen.insert(entry.id) {
                        results.push(entry);
                    }
                }
            }

            // 2) Decrypt-scan sensitive or encrypted entries (only if needed)
            if results.len() < limit as usize {
                let mut cursor_ts = i64::MAX;
                let mut cursor_id = i64::MAX;
                let batch_size = 500;
                let enc_like = format!("{}%", ENCRYPT_PREFIX);
                let sql_sensitive = format!(
                    "SELECT ch.id, ch.content_type, ch.content, ch.html_content, ch.source_app, ch.timestamp, ch.preview, ch.is_pinned, ch.tags, ch.use_count, ch.is_external, ch.pinned_order, ch.source_app_path, ch.content_kinds, ch.ocr_text, ch.ocr_status
	                     FROM clipboard_history ch
                     WHERE (
                         EXISTS (
                             SELECT 1 FROM entry_tags se 
                             WHERE se.entry_id = ch.id 
                               AND se.tag COLLATE NOCASE IN {}
                         )
                         OR ch.content LIKE ?1 
                         OR ch.preview LIKE ?1 
	                         OR ch.html_content LIKE ?1
	                         OR ch.ocr_text LIKE ?1
                     )
                       AND ((ch.timestamp < ?2) OR (ch.timestamp = ?2 AND ch.id < ?3))
                     ORDER BY ch.timestamp DESC, ch.id DESC
                     LIMIT ?4",
                    sensitive_tags_sql
                );

                loop {
                    let mut stmt = conn.prepare(&sql_sensitive).map_err(|e| e.to_string())?;
                    let rows = stmt
                        .query_map(params![enc_like, cursor_ts, cursor_id, batch_size], |row| {
                            let tags_str: String = row.get(8).unwrap_or_else(|_| "[]".to_string());
                            Ok(ClipboardEntry {
                                id: row.get(0)?,
                                content_type: row.get(1)?,
                                content: row.get(2)?, // Encrypted
                                html_content: row.get(3).ok(),
                                source_app: row.get(4)?,
                                timestamp: row.get(5)?,
                                preview: row.get(6)?, // Encrypted
                                is_pinned: row.get::<_, i32>(7)? == 1,
                                tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                                use_count: row.get(9).unwrap_or(0),
                                is_external: row.get::<_, i32>(10)? == 1,
                                pinned_order: row.get(11).unwrap_or(0),
                                source_app_path: row.get(12).unwrap_or(None),
                                file_preview_exists: true,
                                content_kinds: serde_json::from_str(
                                    &row.get::<_, String>(13).unwrap_or_else(|_| "[]".to_string()),
                                )
                                .unwrap_or_default(),
                                ocr_text: row.get(14).ok(),
                                ocr_status: row.get(15).ok(),
                            })
                        })
                        .map_err(|e| e.to_string())?;

                    let mut batch: Vec<ClipboardEntry> = Vec::new();
                    for row in rows {
                        if let Ok(mut entry) = row {
                            entry.content = self.maybe_decrypt_text(&entry.content);
                            entry.preview = self.maybe_decrypt_text(&entry.preview);
                            if let Some(html) = entry.html_content.take() {
                                entry.html_content = Some(self.maybe_decrypt_text(&html));
                            }
                            batch.push(entry);
                        }
                    }

                    if batch.is_empty() {
                        break;
                    }

                    for entry in batch.iter() {
                        let matches = entry.content.to_lowercase().contains(&term)
                            || entry.source_app.to_lowercase().contains(&term)
                            || entry
                                .ocr_text
                                .as_deref()
                                .map(|text| text.to_lowercase().contains(&term))
                                .unwrap_or(false)
                            || entry.tags.iter().any(|t| t.to_lowercase().contains(&term));

                        if matches && seen.insert(entry.id) {
                            results.push(entry.clone());
                            if results.len() >= limit as usize {
                                break;
                            }
                        }
                    }

                    if results.len() >= limit as usize {
                        break;
                    }

                    if let Some(last) = batch.last() {
                        cursor_ts = last.timestamp;
                        cursor_id = last.id;
                    } else {
                        break;
                    }
                }
            }

            results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(b.id.cmp(&a.id)));
            if results.len() > limit as usize {
                results.truncate(limit as usize);
            }
            if let Ok(mut cache) = self.search_cache.lock() {
                cache.put(cache_key, results.clone());
            }
            Ok(results)
        }
    }

    fn delete(&self, id: i64, data_dir: Option<&std::path::Path>) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        self.delete_with_conn(&conn, id, data_dir)
    }

    fn clear(&self, data_dir: Option<&std::path::Path>) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        // Get IDs of unpinned items without tags.
        let mut stmt = conn
            .prepare(
                "SELECT id FROM clipboard_history
             WHERE is_pinned = 0
               AND NOT EXISTS (SELECT 1 FROM entry_tags WHERE entry_id = clipboard_history.id)",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        let ids: Vec<i64> = rows.filter_map(Result::ok).collect();

        // Delete one-by-one so tombstones are recorded for cloud deletion sync.
        for id in &ids {
            self.delete_with_conn(&conn, *id, data_dir)?;
        }

        // VACUUM to reclaim space
        let _ = conn.execute_batch("VACUUM;");
        Ok(())
    }

    fn get_count(&self) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM clipboard_history")
            .map_err(|e| e.to_string())?;
        let count: i64 = stmt
            .query_row([], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        Ok(count)
    }

    fn increment_use_count(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE clipboard_history SET use_count = use_count + 1 WHERE id = ?",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        self.invalidate_caches();
        Ok(())
    }

    fn touch_entry(&self, id: i64, timestamp: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE clipboard_history SET timestamp = ? WHERE id = ?",
            params![timestamp, id],
        )
        .map_err(|e| e.to_string())?;
        self.invalidate_caches();
        Ok(())
    }

    fn toggle_pin(&self, id: i64, is_pinned: bool) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        self.toggle_pin_with_conn(&conn, id, is_pinned)
    }

    fn update_pinned_order(&self, orders: Vec<(i64, i64)>) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        self.update_pinned_order_with_conn(&tx, orders)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_entry_by_id(&self, id: i64) -> Result<Option<ClipboardEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        self.get_entry_by_id_with_conn(&conn, id)
    }

    fn get_entry_by_content(
        &self,
        content: &str,
        content_type: Option<&str>,
    ) -> Result<Option<i64>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        self.find_by_content_with_conn(&conn, content, content_type)
    }

    fn update_entry_content(&self, id: i64, content: &str, preview: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        self.update_entry_content_with_conn(&conn, id, content, preview)
    }

    fn get_entry_content(&self, id: i64) -> Result<Option<String>, String> {
        Ok(self
            .get_entry_content_with_html(id)?
            .map(|(content, _, _)| content))
    }

    fn get_entry_content_full(&self, id: i64) -> Result<Option<(String, String)>, String> {
        Ok(self
            .get_entry_content_with_html(id)?
            .map(|(content, content_type, _)| (content, content_type)))
    }

    fn get_entry_content_with_html(
        &self,
        id: i64,
    ) -> Result<Option<(String, String, Option<String>)>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        self.get_entry_content_with_html_with_conn(&conn, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::repository::migrations::run_migrations;

    fn setup_fts_db() -> Arc<Mutex<Connection>> {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations failed");
        Arc::new(Mutex::new(conn))
    }

    fn insert_entry(conn: &Connection, content: &str, app: &str, ts: i64) {
        let preview: String = content.chars().take(50).collect();
        conn.execute(
            "INSERT INTO clipboard_history
             (content_type, content, html_content, source_app, timestamp, preview,
              is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
              ocr_text, ocr_status)
             VALUES ('text', ?1, NULL, ?2, ?3, ?4, 0, 0, '[]', 0, 0, NULL, NULL, 'pending')",
            params![content, app, ts, preview],
        )
        .expect("insert failed");
    }

    #[test]
    fn test_fts5_creation() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations failed");

        let row: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='clipboard_fts'",
                [],
                |r| r.get(0),
            )
            .expect("clipboard_fts VIRTUAL TABLE must exist after run_migrations");
        assert_eq!(row, "clipboard_fts");

        let trigger_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='trigger' AND name LIKE 'clipboard_history_a%'",
                [],
                |r| r.get(0),
            )
            .expect("trigger count query failed");
        assert_eq!(
            trigger_count, 3,
            "expected INSERT/UPDATE/DELETE triggers on clipboard_history"
        );
    }

    #[test]
    fn test_fts5_insert_trigger() {
        let arc = setup_fts_db();
        let conn = arc.lock().expect("lock");

        insert_entry(
            &conn,
            "the quick brown fox jumps over the lazy dog",
            "Browser",
            1_700_000_000,
        );

        let fts_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_fts", [], |r| r.get(0))
            .expect("clipboard_fts count failed");
        assert_eq!(fts_count, 1, "INSERT trigger must mirror to clipboard_fts");

        let mirrored_content: String = conn
            .query_row("SELECT content FROM clipboard_fts LIMIT 1", [], |r| {
                r.get(0)
            })
            .expect("mirror query failed");
        assert_eq!(
            mirrored_content,
            "the quick brown fox jumps over the lazy dog"
        );
    }

    #[test]
    fn test_fts5_search() {
        let arc = setup_fts_db();
        {
            let conn = arc.lock().expect("lock");
            insert_entry(
                &conn,
                "alpha apple banana foo cherry",
                "App1",
                1_700_000_000,
            );
            insert_entry(&conn, "delta elephant falcon grape", "App2", 1_700_000_001);
            insert_entry(&conn, "hello foo world baz qux", "App3", 1_700_000_002);
        }

        let repo = SqliteClipboardRepository::new(arc);
        let results = repo.search_fts("foo", 10).expect("search_fts failed");

        assert_eq!(
            results.len(),
            2,
            "expected exactly 2 entries containing 'foo'"
        );
        let contents: Vec<&str> = results.iter().map(|e| e.content.as_str()).collect();
        assert!(contents.iter().any(|c| c.contains("apple banana foo")));
        assert!(contents.iter().any(|c| c.contains("hello foo world")));
    }

    #[test]
    fn test_fts5_search_returns_image_by_ocr_text() {
        let arc = setup_fts_db();
        {
            let conn = arc.lock().expect("lock");
            conn.execute(
                "INSERT INTO clipboard_history
                 (content_type, content, html_content, source_app, timestamp, preview,
                  is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
                  content_kinds, ocr_text, ocr_status)
                 VALUES ('image', 'data:image/png;base64,AAAA', NULL, 'Screenshot', 1700000003,
                         'screenshot image', 0, 0, '[]', 0, 0, NULL, '[]',
                         'Bryobacterales bacterium annotation panel', 'done')",
                [],
            )
            .expect("insert image row");
        }

        let repo = SqliteClipboardRepository::new(arc);
        let results = repo
            .search_fts("Bryobacterales", 10)
            .expect("search_fts failed");

        assert_eq!(results.len(), 1, "OCR text must make image searchable");
        assert_eq!(results[0].content_type, "image");
        assert_eq!(
            results[0].ocr_text.as_deref(),
            Some("Bryobacterales bacterium annotation panel")
        );
    }

    #[test]
    fn test_fts5_search_maps_every_selected_column() {
        let arc = setup_fts_db();
        {
            let conn = arc.lock().expect("lock");
            conn.execute(
                "INSERT INTO clipboard_history
                 (content_type, content, html_content, source_app, timestamp, preview,
                  is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
                  content_kinds, ocr_text, ocr_status)
                 VALUES ('text', 'quantum entanglement notes', '<p>quantum</p>', 'Editor.exe', 1700000009,
                         'quantum entanglement', 0, 0, '[\"work\"]', 0, 0, 'C:/apps/editor.exe',
                         '[\"code\",\"url\"]', 'quantum scanned body', 'done')",
                [],
            )
            .expect("insert fully populated row");
        }

        let repo = SqliteClipboardRepository::new(arc);
        let results = repo
            .search_fts("quantum", 10)
            .expect("search_fts failed");

        assert_eq!(results.len(), 1);
        let hit = &results[0];
        assert_eq!(hit.content_type, "text");
        assert_eq!(hit.content, "quantum entanglement notes");
        assert_eq!(hit.html_content.as_deref(), Some("<p>quantum</p>"));
        assert_eq!(hit.source_app, "Editor.exe");
        assert_eq!(hit.preview, "quantum entanglement");
        assert_eq!(hit.tags, vec!["work".to_string()]);
        assert_eq!(
            hit.source_app_path.as_deref(),
            Some("C:/apps/editor.exe"),
            "the column after pinned_order must still resolve to source_app_path"
        );
        assert_eq!(
            hit.content_kinds,
            vec!["code".to_string(), "url".to_string()],
            "the column before ocr_text must still resolve to content_kinds"
        );
        assert_eq!(hit.ocr_text.as_deref(), Some("quantum scanned body"));
        assert_eq!(
            hit.ocr_status.as_deref(),
            Some("done"),
            "the final selected column must still resolve to ocr_status"
        );
    }

    #[test]
    fn test_fts5_unicode() {
        let arc = setup_fts_db();
        {
            let conn = arc.lock().expect("lock");
            insert_entry(&conn, "Rust 是一门系统编程语言", "AppRust", 1_700_000_000);
            insert_entry(&conn, "你好世界 欢迎使用 DezirClip", "AppCJK", 1_700_000_001);
            insert_entry(
                &conn,
                "Memory safety 🎉 zero-cost abstractions",
                "AppEmoji",
                1_700_000_002,
            );
            insert_entry(
                &conn,
                "plain ascii clipboard entry",
                "AppAscii",
                1_700_000_003,
            );
        }

        let repo = SqliteClipboardRepository::new(arc);

        let cjk_results = repo
            .search_fts("你好世", 10)
            .expect("CJK search_fts failed");
        assert_eq!(
            cjk_results.len(),
            1,
            "CJK 3-char query '你好世' (trigram minimum) must match exactly 1 row"
        );
        assert!(cjk_results[0].content.contains("你好世界"));

        let ascii_results = repo
            .search_fts("Rust", 10)
            .expect("ASCII search_fts failed");
        assert!(
            ascii_results
                .iter()
                .any(|e| e.content.contains("Rust 是一门")),
            "ASCII 'Rust' should match the CJK+ASCII row"
        );

        let emoji_results = repo
            .search_fts("safety", 10)
            .expect("emoji-row search_fts failed");
        assert_eq!(emoji_results.len(), 1);
        assert!(emoji_results[0].content.contains("🎉"));
    }

    fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .expect("table_info prepare");
        let mut rows = stmt.query([]).expect("table_info query");
        while let Some(row) = rows.next().expect("row iter") {
            let name: String = row.get(1).expect("name col");
            if name == column {
                return true;
            }
        }
        false
    }

    fn has_index(conn: &Connection, index_name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type='index' AND name=?",
            [index_name],
            |_| Ok(()),
        )
        .is_ok()
    }

    #[test]
    fn test_v13_adds_content_kinds_column() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("initial migrations failed");

        conn.execute(
            "DROP INDEX IF EXISTS idx_clipboard_history_content_kinds",
            [],
        )
        .expect("drop index");
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_ai", [])
            .expect("drop ai trigger");
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_ad", [])
            .expect("drop ad trigger");
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_au", [])
            .expect("drop au trigger");
        conn.execute("DROP TABLE IF EXISTS clipboard_fts", [])
            .expect("drop fts table");
        // Roll the whole tail back rather than naming the versions that existed
        // when this test was written: the migration gate is `MAX(version)`, so
        // a leftover later marker would skip the block this test re-runs.
        conn.execute(
            "DELETE FROM schema_migrations WHERE version >= 13",
            [],
        )
        .expect("version reset failed");
        conn.execute(
            "ALTER TABLE clipboard_history DROP COLUMN content_kinds",
            [],
        )
        .expect("DROP COLUMN requires SQLite >= 3.35; rusqlite 0.31 bundled satisfies this");

        assert!(
            !has_column(&conn, "clipboard_history", "content_kinds"),
            "pre-v13 state: content_kinds column must be missing"
        );

        run_migrations(&mut conn).expect("re-applied migrations failed");

        assert!(
            has_column(&conn, "clipboard_history", "content_kinds"),
            "post-v13 state: content_kinds column must exist after migration"
        );

        let dflt_value: String = conn
            .query_row(
                "SELECT dflt_value FROM pragma_table_info('clipboard_history') WHERE name='content_kinds'",
                [],
                |row| row.get(0),
            )
            .expect("column metadata query");
        assert_eq!(
            dflt_value, "'[]'",
            "content_kinds default must be the JSON array literal '[]'"
        );
    }

    #[test]
    fn test_v13_indexes_exist() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations failed");

        assert!(
            has_index(&conn, "idx_clipboard_history_content_kinds"),
            "idx_clipboard_history_content_kinds must exist after v13"
        );
    }

    #[test]
    fn test_v13_fts5_rebuild() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations failed");

        assert!(
            has_column(&conn, "clipboard_fts", "content_kinds"),
            "v13 FTS5 schema must include content_kinds column"
        );

        let arc = Arc::new(Mutex::new(conn));
        {
            let conn = arc.lock().expect("lock");
            insert_entry(
                &conn,
                "alpha apple banana foo cherry",
                "App1",
                1_700_000_000,
            );
            insert_entry(&conn, "delta elephant falcon grape", "App2", 1_700_000_001);
            insert_entry(&conn, "hello foo world baz qux", "App3", 1_700_000_002);
        }

        let repo = SqliteClipboardRepository::new(arc);
        let results = repo.search_fts("foo", 10).expect("search_fts failed");
        assert_eq!(
            results.len(),
            2,
            "v13 FTS5 must still match both 'foo'-bearing entries after rebuild"
        );
        let contents: Vec<&str> = results.iter().map(|e| e.content.as_str()).collect();
        assert!(contents.iter().any(|c| c.contains("apple banana foo")));
        assert!(contents.iter().any(|c| c.contains("hello foo world")));
    }

    // The dedup stage hands its computed hash down through
    // `save_with_conn_and_image_hash`, and the row must be stored under exactly
    // the value that lookup matched on. If the two ever diverged, a pasted
    // picture would look brand new on every paste and the history would fill
    // with duplicates.
    #[test]
    fn handed_in_hash_is_stored_as_content_hash() {
        use crate::infrastructure::repository::migrations::run_migrations;
        use base64::Engine;
        use image::ImageEncoder;

        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &[255, 0, 0, 255],
                1,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .expect("png encode");
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        let dedup_hash = calc_image_hash(&url).expect("png payload should hash");

        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations failed");
        let repo = SqliteClipboardRepository::new(Arc::new(Mutex::new(conn)));

        let entry = ClipboardEntry {
            id: 0,
            content_type: "image".to_string(),
            content: url,
            html_content: None,
            source_app: "Test".to_string(),
            source_app_path: None,
            timestamp: 1_700_000_000,
            preview: "[Image Content]".to_string(),
            is_pinned: false,
            tags: Vec::new(),
            use_count: 0,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: true,
            content_kinds: Vec::new(),
            ocr_text: None,
            ocr_status: None,
        };

        let conn = repo.conn.lock().expect("lock");
        let new_id = repo
            .save_with_conn_and_image_hash(&conn, &entry, None, Some(dedup_hash))
            .expect("save with handed-in hash");
        let old_id = repo.save_with_conn(&conn, &entry, None).expect("save recomputing");

        let read_hash = |id: i64| -> i64 {
            conn.query_row(
                "SELECT content_hash FROM clipboard_history WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .expect("row should exist")
        };

        assert_eq!(
            read_hash(new_id),
            read_hash(old_id),
            "reusing the dedup hash must store the same content_hash as recomputing it"
        );
        assert_eq!(read_hash(new_id), dedup_hash);
    }

    // `save_with_conn_and_image_hash` took the content as a `Cow` so that a
    // screenshot's 5.3 MB data URL is never copied on the way to becoming a short
    // file path. The borrow only works because every branch that still holds the
    // caller's string hands it back intact, and the branch that replaces it hands
    // back an owned path instead. These pin both directions of that, plus the
    // encrypt branch, which is the one place the three payloads stop being
    // borrowed and start being rebuilt.
    fn png_data_url() -> String {
        use base64::Engine;
        use image::ImageEncoder;

        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &[12, 34, 56, 255],
                1,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .expect("png encode");
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        )
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dz-repo-test-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn image_entry(content: String) -> ClipboardEntry {
        ClipboardEntry {
            id: 0,
            content_type: "image".to_string(),
            content,
            html_content: None,
            source_app: "Test".to_string(),
            source_app_path: None,
            timestamp: 1_700_000_000,
            preview: "[Image Content]".to_string(),
            is_pinned: false,
            tags: Vec::new(),
            use_count: 0,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: true,
            content_kinds: Vec::new(),
            ocr_text: None,
            ocr_status: None,
        }
    }

    #[test]
    fn image_is_replaced_by_a_file_path_when_a_data_dir_is_given() {
        let arc = setup_fts_db();
        let repo = SqliteClipboardRepository::new(arc);
        let dir = scratch_dir("externalize-ok");
        let url = png_data_url();

        let conn = repo.conn.lock().expect("lock");
        let id = repo
            .save_with_conn_and_image_hash(&conn, &image_entry(url.clone()), Some(&dir), None)
            .expect("save should externalize");

        let (content, is_external): (String, i64) = conn
            .query_row(
                "SELECT content, is_external FROM clipboard_history WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row should exist");

        assert!(
            !content.starts_with("data:"),
            "a successful externalization must not keep the data URL, got {} bytes",
            content.len()
        );
        assert!(
            std::path::Path::new(&content).exists(),
            "the stored path must point at a real file: {}",
            content
        );
        assert_eq!(is_external, 1, "an externalized image must be marked external");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn image_keeps_its_data_url_when_no_data_dir_is_given() {
        let arc = setup_fts_db();
        let repo = SqliteClipboardRepository::new(arc);
        let url = png_data_url();

        let conn = repo.conn.lock().expect("lock");
        let id = repo
            .save_with_conn_and_image_hash(&conn, &image_entry(url.clone()), None, None)
            .expect("save without a data dir");

        let (content, is_external): (String, i64) = conn
            .query_row(
                "SELECT content, is_external FROM clipboard_history WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row should exist");

        assert_eq!(content, url, "with no data dir the caller's string is stored as is");
        assert_eq!(is_external, 0, "a non-externalized image must not claim to be");
    }

    #[test]
    fn image_keeps_its_data_url_when_externalization_fails() {
        let arc = setup_fts_db();
        let repo = SqliteClipboardRepository::new(arc);
        let dir = scratch_dir("externalize-fail");
        // `save_image_to_file` returns None when the payload is not decodable
        // base64, which is the same "no path produced" the borrow has to survive.
        let broken = "data:image/png;base64,!!!!not base64!!!!".to_string();

        let conn = repo.conn.lock().expect("lock");
        let id = repo
            .save_with_conn_and_image_hash(&conn, &image_entry(broken.clone()), Some(&dir), None)
            .expect("a failed externalization must still save");

        let (content, is_external): (String, i64) = conn
            .query_row(
                "SELECT content, is_external FROM clipboard_history WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row should exist");

        assert_eq!(
            content, broken,
            "when no file path is produced the borrowed content must survive"
        );
        assert_eq!(is_external, 0);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sensitive_text_goes_through_the_encrypt_branch_and_round_trips() {
        let arc = setup_fts_db();
        let repo = SqliteClipboardRepository::new(arc);
        let mut entry = weighted_entry(64);
        entry.content_type = "text".to_string();
        entry.content = "correct horse battery staple".to_string();
        entry.preview = "correct horse".to_string();
        entry.tags = vec!["sensitive".to_string()];

        let conn = repo.conn.lock().expect("lock");
        let id = repo
            .save_with_conn_and_image_hash(&conn, &entry, None, None)
            .expect("save sensitive entry");

        let stored: String = conn
            .query_row(
                "SELECT content FROM clipboard_history WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .expect("row should exist");

        assert_eq!(
            repo.maybe_decrypt_text(&stored),
            entry.content,
            "the encrypt branch must hand back exactly what it was given"
        );
        // DPAPI on Windows and a generated master key elsewhere both encrypt; if
        // neither is available the branch is a passthrough and there is nothing
        // left to assert beyond the round trip above.
        if crate::infrastructure::encryption::encrypt_value(&entry.content).is_some() {
            assert!(
                crate::infrastructure::encryption::is_encrypted_value(&stored),
                "a sensitive entry must not be readable in the database"
            );
        }
    }

    // The repository caches hold pages of entries whose payload size is decided
    // by whatever the user happened to copy, so an entry-count ceiling on its own
    // leaves the resident set unbounded. These pin the byte ceiling: it has to
    // bind before the count ceiling does, it has to bind by recency, and the
    // running total must never drift from what the cache is actually holding.

    fn weighted_entry(content_len: usize) -> ClipboardEntry {
        ClipboardEntry {
            id: 0,
            content_type: "image".to_string(),
            content: "x".repeat(content_len),
            html_content: None,
            source_app: "Test".to_string(),
            source_app_path: None,
            timestamp: 1_700_000_000,
            preview: String::new(),
            is_pinned: false,
            tags: Vec::new(),
            use_count: 0,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: true,
            content_kinds: Vec::new(),
            ocr_text: None,
            ocr_status: None,
        }
    }

    /// A page of `entries` entries carrying `payload_bytes` of content in total,
    /// so a test can state a weight in round numbers without depending on the
    /// weight function's internals.
    fn page_of(payload_bytes: usize, entries: usize) -> Vec<ClipboardEntry> {
        let per_entry = payload_bytes / entries;
        (0..entries).map(|_| weighted_entry(per_entry)).collect()
    }

    #[test]
    fn byte_ceiling_evicts_long_before_the_entry_ceiling_would() {
        let mut cache: SimpleLruCache<Vec<ClipboardEntry>> =
            SimpleLruCache::new(64, 1_000, entry_page_weight);

        for page in 0..10 {
            cache.put(format!("page:{page}"), page_of(400, 2));
        }

        assert!(
            cache.map.len() < 10,
            "ten ~418-byte pages blow a 1000-byte ceiling while sitting far below the \
             64-entry ceiling, so the byte ceiling must have bound: {} pages retained",
            cache.map.len()
        );
        assert!(cache.weight <= 1_000);
    }

    #[test]
    fn byte_eviction_drops_the_least_recently_used_page() {
        let mut cache: SimpleLruCache<Vec<ClipboardEntry>> =
            SimpleLruCache::new(64, 1_000, entry_page_weight);
        cache.put("a".to_string(), page_of(400, 2));
        cache.put("b".to_string(), page_of(400, 2));
        assert!(cache.get("a").is_some());
        cache.put("c".to_string(), page_of(400, 2));

        assert!(
            cache.get("a").is_some(),
            "the page that was just read must survive the next insert"
        );
        assert!(
            cache.get("b").is_none(),
            "the untouched page is the least recently used and must be the one to go"
        );
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn a_page_too_big_for_the_whole_ceiling_is_not_retained() {
        let mut cache: SimpleLruCache<Vec<ClipboardEntry>> =
            SimpleLruCache::new(64, 1_000, entry_page_weight);
        cache.put("huge".to_string(), page_of(50_000, 1));

        assert!(
            cache.map.is_empty(),
            "keeping a page that alone exceeds the ceiling would leave the cache holding \
             exactly the payload the ceiling exists to exclude"
        );
        assert_eq!(cache.weight, 0);
    }

    #[test]
    fn the_entry_ceiling_still_applies_to_pages_that_cost_nothing() {
        let mut cache: SimpleLruCache<Vec<ClipboardEntry>> =
            SimpleLruCache::new(4, 1_000_000, entry_page_weight);
        for page in 0..20 {
            cache.put(format!("page:{page}"), Vec::new());
        }

        assert_eq!(cache.map.len(), 4, "a zero-byte page must not escape the count ceiling");
    }

    #[test]
    fn retained_weight_tracks_the_entries_actually_held() {
        let mut cache: SimpleLruCache<Vec<ClipboardEntry>> =
            SimpleLruCache::new(64, 10_000, entry_page_weight);
        for page in 0..5 {
            cache.put(format!("page:{page}"), page_of(200, 2));
        }

        let recomputed: usize = cache.map.values().map(|v| entry_page_weight(v)).sum();
        assert_eq!(
            cache.weight, recomputed,
            "the running total must match the live entries, or eviction starts guessing"
        );
    }

    #[test]
    fn replacing_a_key_corrects_the_weight_instead_of_adding_to_it() {
        let mut cache: SimpleLruCache<Vec<ClipboardEntry>> =
            SimpleLruCache::new(64, 10_000, entry_page_weight);
        cache.put("k".to_string(), page_of(1_000, 1));
        let after_large = cache.weight;

        cache.put("k".to_string(), page_of(10, 1));

        assert_eq!(cache.map.len(), 1, "replacing a key must not add a second entry");
        assert!(
            after_large > cache.weight,
            "replacing a heavy page with a light one must shrink the total, not stack on it"
        );
        assert_eq!(cache.weight, entry_page_weight(&page_of(10, 1)));
    }

    #[test]
    fn clearing_resets_the_weight_along_with_the_entries() {
        let mut cache: SimpleLruCache<Vec<ClipboardEntry>> =
            SimpleLruCache::new(64, 10_000, entry_page_weight);
        cache.put("a".to_string(), page_of(400, 2));
        assert!(cache.weight > 0);

        cache.clear();

        assert_eq!(
            cache.weight, 0,
            "a stale total after an invalidation would evict healthy entries forever"
        );
    }

    #[test]
    fn html_and_ocr_payload_count_towards_the_page_weight() {
        let mut entry = weighted_entry(10);
        let bare = entry_payload_weight(&entry);

        entry.html_content = Some("x".repeat(500));
        entry.ocr_text = Some("y".repeat(300));

        assert_eq!(
            entry_payload_weight(&entry) - bare,
            800,
            "an entry that also carries HTML and OCR text must weigh what it holds, \
             not just its content column"
        );
    }

    #[test]
    fn the_history_cache_stays_accounted_after_real_reads() {
        let arc = setup_fts_db();
        {
            let conn = arc.lock().expect("lock");
            for i in 0..120 {
                insert_entry(&conn, &"body ".repeat(300), "Browser", 1_700_000_000 + i);
            }
        }

        let repo = SqliteClipboardRepository::new(arc);
        for offset in [0, 20, 40, 0] {
            repo.get_history(20, offset, None).expect("get_history failed");
        }

        let cached = repo.history_cache.lock().expect("lock");
        let recomputed: usize = cached.map.values().map(|v| entry_page_weight(v)).sum();
        assert_eq!(cached.weight, recomputed);
        assert!(cached.weight <= HISTORY_CACHE_MAX_BYTES);
    }
}
