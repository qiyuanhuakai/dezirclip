use crate::database::{
    calc_image_hash, calc_text_hash, has_sensitive_tag, is_text_type, save_image_to_file,
    ENCRYPT_PREFIX,
};
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::encryption;
use crate::infrastructure::repository::settings_repo::SqliteSettingsRepository;
use rusqlite::params;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use urlencoding::decode;

const RICH_IMAGE_FALLBACK_PREFIX: &str = "<!--TIEZ_RICH_IMAGE:";
const RICH_IMAGE_FALLBACK_SUFFIX: &str = "-->";
const HISTORY_CONTENT_PREVIEW_CHARS: usize = 2_000;
const HISTORY_PREVIEW_CHARS: usize = 500;
const HISTORY_HTML_PREVIEW_CHARS: usize = 5_000;
const HISTORY_LIST_SELECT_COLUMNS: &str = "id, content_type, \
    CASE WHEN content LIKE 'linux:%' OR content LIKE 'dpapi:%' THEN content ELSE substr(content, 1, 2004) END, \
    CASE WHEN html_content LIKE 'linux:%' OR html_content LIKE 'dpapi:%' THEN html_content ELSE substr(html_content, 1, 5004) END, \
    source_app, timestamp, \
    CASE WHEN preview LIKE 'linux:%' OR preview LIKE 'dpapi:%' THEN preview ELSE substr(preview, 1, 504) END, \
    is_pinned, tags, use_count, is_external, pinned_order, source_app_path";

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

struct SimpleLruCache<T: Clone> {
    map: HashMap<String, T>,
    order: VecDeque<String>,
    capacity: usize,
}

impl<T: Clone> SimpleLruCache<T> {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
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
        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            self.touch(&key);
            return;
        }
        self.map.insert(key.clone(), value);
        self.order.push_back(key);
        while self.map.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
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
            history_cache: Arc::new(Mutex::new(SimpleLruCache::new(64))),
            search_cache: Arc::new(Mutex::new(SimpleLruCache::new(64))),
            content_cache: Arc::new(Mutex::new(SimpleLruCache::new(256))),
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
        // Encrypt only when explicitly marked as sensitive
        let should_encrypt = has_sensitive_tag(&entry.tags);

        let mut final_content = entry.content.clone();
        let mut final_is_external = entry.is_external;

        // Externalize image if possible
        if entry.content_type == "image" && entry.content.starts_with("data:image/") {
            if let Some(dir) = data_dir {
                if let Some(path) = save_image_to_file(&entry.content, dir) {
                    final_content = path;
                    final_is_external = true;
                }
            }
        }

        let calculated_hash = if entry.content_type == "image" {
            if entry.content.starts_with("data:") {
                calc_image_hash(&entry.content).unwrap_or(0)
            } else {
                if let Ok(img) = image::open(&entry.content) {
                    let thumb = img.resize_exact(32, 32, image::imageops::FilterType::Nearest);
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    thumb.as_bytes().hash(&mut hasher);
                    hasher.finish() as i64
                } else {
                    0
                }
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
                final_content,
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
        if content_type == Some("image") {
            if let Some(hash) = calc_image_hash(content) {
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
                // Count non-pinned entries that have no tags
                let count: i32 = conn.query_row(
                    "SELECT COUNT(*) FROM clipboard_history WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL)",
                    [],
                    |row| row.get(0)
                ).map_err(|e| e.to_string())?;

                if count > limit {
                    // First, get the IDs that will be deleted
                    let to_delete = count - limit;
                    let deleted_ids: Vec<i64> = {
                        let mut stmt = conn
                            .prepare(
                                "SELECT id FROM clipboard_history
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
            "SELECT id, content_type, content, html_content, source_app, timestamp, preview, is_pinned, tags, use_count, is_external, pinned_order, source_app_path 
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
                ocr_text: None,
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
            .prepare(
                "SELECT ocr_status, ocr_text FROM clipboard_history WHERE id = ? LIMIT 1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(params![id])
            .map_err(|e| e.to_string())?;
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
                        snippet(clipboard_fts, 0, '<mark>', '</mark>', '...', 16),
                        highlight(clipboard_fts, 0, '<mark>', '</mark>')
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

                let _snippet: String = row.get(13)?;
                let _highlight: String = row.get(14)?;

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
                    content_kinds: Vec::new(),
                    ocr_text: None,
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
                    content_kinds: Vec::new(),
                    ocr_text: None,
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
                "SELECT DISTINCT ch.id, ch.content_type, ch.content, ch.html_content, ch.source_app, ch.timestamp, ch.preview, ch.is_pinned, ch.tags, ch.use_count, ch.is_external, ch.pinned_order, ch.source_app_path 
                 FROM clipboard_history ch
                 LEFT JOIN entry_tags et ON ch.id = et.entry_id
                 WHERE ch.content LIKE '%' || ? || '%' 
                    OR ch.source_app LIKE '%' || ? || '%' 
                    OR et.tag LIKE '%' || ? || '%'
                 ORDER BY ch.timestamp DESC 
                 LIMIT ?",
            ).map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map(params![term, term, term, limit], |row| {
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
                        ocr_text: None,
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
                "SELECT DISTINCT ch.id, ch.content_type, ch.content, ch.html_content, ch.source_app, ch.timestamp, ch.preview, ch.is_pinned, ch.tags, ch.use_count, ch.is_external, ch.pinned_order, ch.source_app_path 
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
                        content_kinds: Vec::new(),
                        ocr_text: None,
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
                    "SELECT ch.id, ch.content_type, ch.content, ch.html_content, ch.source_app, ch.timestamp, ch.preview, ch.is_pinned, ch.tags, ch.use_count, ch.is_external, ch.pinned_order, ch.source_app_path 
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
                                content_kinds: Vec::new(),
                                ocr_text: None,
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

        insert_entry(&conn, "the quick brown fox jumps over the lazy dog", "Browser", 1_700_000_000);

        let fts_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_fts", [], |r| r.get(0))
            .expect("clipboard_fts count failed");
        assert_eq!(fts_count, 1, "INSERT trigger must mirror to clipboard_fts");

        let mirrored_content: String = conn
            .query_row("SELECT content FROM clipboard_fts LIMIT 1", [], |r| r.get(0))
            .expect("mirror query failed");
        assert_eq!(mirrored_content, "the quick brown fox jumps over the lazy dog");
    }

    #[test]
    fn test_fts5_search() {
        let arc = setup_fts_db();
        {
            let conn = arc.lock().expect("lock");
            insert_entry(&conn, "alpha apple banana foo cherry", "App1", 1_700_000_000);
            insert_entry(&conn, "delta elephant falcon grape", "App2", 1_700_000_001);
            insert_entry(&conn, "hello foo world baz qux", "App3", 1_700_000_002);
        }

        let repo = SqliteClipboardRepository::new(arc);
        let results = repo.search_fts("foo", 10).expect("search_fts failed");

        assert_eq!(results.len(), 2, "expected exactly 2 entries containing 'foo'");
        let contents: Vec<&str> = results.iter().map(|e| e.content.as_str()).collect();
        assert!(contents.iter().any(|c| c.contains("apple banana foo")));
        assert!(contents.iter().any(|c| c.contains("hello foo world")));
    }

    #[test]
    fn test_fts5_unicode() {
        let arc = setup_fts_db();
        {
            let conn = arc.lock().expect("lock");
            insert_entry(&conn, "Rust 是一门系统编程语言", "AppRust", 1_700_000_000);
            insert_entry(&conn, "你好世界 欢迎使用 TieZ", "AppCJK", 1_700_000_001);
            insert_entry(
                &conn,
                "Memory safety 🎉 zero-cost abstractions",
                "AppEmoji",
                1_700_000_002,
            );
            insert_entry(&conn, "plain ascii clipboard entry", "AppAscii", 1_700_000_003);
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
            ascii_results.iter().any(|e| e.content.contains("Rust 是一门")),
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

        conn.execute("DROP INDEX IF EXISTS idx_clipboard_history_content_kinds", [])
            .expect("drop index");
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_ai", [])
            .expect("drop ai trigger");
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_ad", [])
            .expect("drop ad trigger");
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_au", [])
            .expect("drop au trigger");
        conn.execute("DROP TABLE IF EXISTS clipboard_fts", [])
            .expect("drop fts table");
        conn.execute(
            "DELETE FROM schema_migrations WHERE version IN (13, 14)",
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
            insert_entry(&conn, "alpha apple banana foo cherry", "App1", 1_700_000_000);
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
}
