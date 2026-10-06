use crate::app_state::{AppDataDir, PasteQueue, SessionHistory, SettingsState};
use crate::database::is_text_type;
use crate::database::DbState;
use crate::domain::models::ClipboardEntry;
use crate::global_state::IS_DESTROYED;
#[cfg(not(target_os = "windows"))]
use crate::infrastructure::linux_api::window_tracker::{
    get_active_app_info as get_clipboard_source_app_info, ActiveAppInfo,
};
#[cfg(target_os = "windows")]
use crate::infrastructure::windows_api::window_tracker::{
    get_clipboard_source_app_info, ActiveAppInfo,
};
use crate::services::clipboard::utils::*;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const MAX_PERSISTED_TEXT_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum ClipboardData {
    Text(String),
    RichText { text: String, html: String },
    Image { data_url: String },
    Files(Vec<String>),
}

pub struct PipelineContext {
    /// The captured payload, owned outright. Only the first stage reads it, and it
    /// hands the strings straight to the entry it builds -- an image capture's
    /// data URL is megabytes and a text capture is capped at ten, so copying any
    /// of it is copying the whole thing. `Option` is what makes "consumed here"
    /// say so in the type rather than in a comment.
    pub data: Option<ClipboardData>,
    pub app_handle: AppHandle,
    pub source_app: String,
    pub source_app_path: Option<String>,
    pub timestamp: i64,
    pub entry: Option<ClipboardEntry>,
    pub should_stop: bool,
    pub pending_removals: Vec<i64>,
    pub reuse_session_id: Option<i64>,
    /// `content_hash` for an image entry, once the dedup stage has decoded the
    /// picture to look one up. The persistence stage writes the very same value
    /// to the database, so it is handed over instead of decoded a second time.
    /// `None` until the dedup stage runs, and for a non-deduped run.
    pub image_hash: Option<i64>,
}

impl PipelineContext {
    pub fn new(
        app_handle: AppHandle,
        data: ClipboardData,
        source_snapshot: Option<ActiveAppInfo>,
    ) -> Self {
        let active_app = source_snapshot.unwrap_or_else(get_clipboard_source_app_info);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        Self {
            data: Some(data),
            app_handle,
            source_app: active_app.app_name,
            source_app_path: active_app.process_path,
            timestamp,
            entry: None,
            should_stop: false,
            pending_removals: Vec::new(),
            reuse_session_id: None,
            image_hash: None,
        }
    }
}

pub trait PipelineStage {
    fn process(&self, context: &mut PipelineContext);
}

pub struct ClipboardPipeline {
    stages: Vec<Box<dyn PipelineStage + Send + Sync>>,
}

impl ClipboardPipeline {
    pub fn new() -> Self {
        Self {
            stages: vec![
                Box::new(DiscoveryStage),
                Box::new(TransformationStage),
                Box::new(ValidationStage),
                Box::new(PersistenceStage),
                Box::new(DistributionStage),
            ],
        }
    }

    pub fn execute(&self, context: &mut PipelineContext) {
        for stage in &self.stages {
            stage.process(context);
            if context.should_stop {
                break;
            }
        }
    }
}

// Stage 1: Discovery
/// What a single copied file becomes. Anything that is not a picture or a video
/// stays a plain file entry, which is what the list renders it as either way.
fn classify_file_path(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".gif")
        || lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".bmp")
        || lower.ends_with(".webp")
    {
        "image"
    } else if lower.ends_with(".mp4")
        || lower.ends_with(".mkv")
        || lower.ends_with(".avi")
        || lower.ends_with(".mov")
        || lower.ends_with(".wmv")
        || lower.ends_with(".flv")
        || lower.ends_with(".webm")
    {
        "video"
    } else {
        "file"
    }
}

pub struct DiscoveryStage;
impl PipelineStage for DiscoveryStage {
    fn process(&self, ctx: &mut PipelineContext) {
        // Taken by value: every arm below hands the payload straight to the entry
        // instead of copying it. Discovery is the first stage and the only reader,
        // so the strings it moves are the only copies of a capture that exist.
        let data = ctx
            .data
            .take()
            .expect("discovery runs first and is the only reader of the payload");
        let (content_type, content, html_content) = match data {
            ClipboardData::Text(t) => (detect_content_type(&t), t, None),
            ClipboardData::RichText { text, html } => {
                ("rich_text".to_string(), text, Some(html))
            }
            ClipboardData::Image { data_url } => ("image".to_string(), data_url, None),
            ClipboardData::Files(mut f) => {
                // A single path never needs the join: on one element `join`
                // returns that element, and every branch used either the path or
                // that same value. Only the multi-file case actually concatenates.
                if f.len() == 1 {
                    let path = f.pop().expect("length checked above");
                    (classify_file_path(&path).to_string(), path, None)
                } else {
                    ("file".to_string(), f.join("\n"), None)
                }
            }
        };

        let preview = if content_type == "image" {
            "[Image Content]".to_string()
        } else if content.chars().count() > 500 {
            let preview_text: String = content.chars().take(497).collect();
            format!("{}...", preview_text.replace('\n', " "))
        } else {
            content.replace('\n', " ")
        };

        let is_external =
            (content_type == "file" || content_type == "video" || content_type == "image")
                && !content.starts_with("data:");

        ctx.entry = Some(ClipboardEntry {
            id: 0,
            content_type,
            content,
            html_content,
            source_app: ctx.source_app.clone(),
            source_app_path: ctx.source_app_path.clone(),
            timestamp: ctx.timestamp,
            preview,
            is_pinned: false,
            tags: Vec::new(),
            use_count: 0,
            is_external,
            pinned_order: 0,
            file_preview_exists: true,
            content_kinds: Vec::new(),
            ocr_text: None,
            ocr_status: None,
        });
    }
}

// Stage 2: Transformation
pub struct TransformationStage;
impl PipelineStage for TransformationStage {
    fn process(&self, ctx: &mut PipelineContext) {
        let entry = ctx.entry.as_mut().unwrap();
        let settings = ctx.app_handle.state::<SettingsState>();

        // Normalization (already partially done but let's be thorough).
        // Only rebuild the string when there is actually something to change:
        // this runs for every content type, and an image capture carries a
        // multi-megabyte data URL that is already trimmed and LF-only.
        let trimmed = entry.content.trim();
        if trimmed.len() != entry.content.len() || trimmed.contains("\r\n") {
            entry.content = trimmed.replace("\r\n", "\n");
        }

        // Sensitive Info
        let protect_kinds = settings.privacy_protection_kinds.lock().unwrap().clone();
        let custom_rules = settings
            .privacy_protection_custom_rules
            .lock()
            .unwrap()
            .clone();
        if settings.privacy_protection.load(Ordering::Relaxed) && is_text_type(&entry.content_type)
        {
            if contains_sensitive_info(&entry.content, &protect_kinds, &custom_rules) {
                entry.tags.push("sensitive".to_string());
            }
        }

        // Rich Text Image Processing
        if let Some(html) = &entry.html_content {
            let app_data_dir = ctx.app_handle.state::<AppDataDir>();
            let data_dir = app_data_dir.0.lock().unwrap().clone();

            entry.html_content = if settings.persistent.load(Ordering::Relaxed) {
                let html_with_local_assets = process_local_images_in_html(html, &data_dir);
                Some(externalize_rich_image_fallback(
                    &html_with_local_assets,
                    &data_dir,
                ))
            } else {
                Some(embed_local_images(html))
            };
        }
    }
}

// Stage 3: Validation (Deduplication & Sequential Echo)
pub struct ValidationStage;
impl ValidationStage {
    fn should_stop_for_sequential_echo(&self, ctx: &PipelineContext) -> bool {
        let settings = ctx.app_handle.state::<SettingsState>();
        if !settings.sequential_mode.load(Ordering::Relaxed) {
            return false;
        }
        let entry = ctx.entry.as_ref().unwrap();
        let queue_state = ctx.app_handle.state::<PasteQueue>();
        let queue = queue_state.0.lock().unwrap();
        queue.last_action_was_paste && queue.last_pasted_content.as_deref() == Some(&entry.content)
    }

    fn process_deduplication(&self, ctx: &mut PipelineContext) {
        let settings = ctx.app_handle.state::<SettingsState>();
        if !settings.deduplicate.load(Ordering::Relaxed) {
            return;
        }

        let persistent_enabled = settings.persistent.load(Ordering::Relaxed);
        let db_state = ctx.app_handle.state::<DbState>();
        let conn = db_state.conn.lock().unwrap();

        let mut existing_id = None;
        let htmls_equivalent = |a: Option<&str>, b: Option<&str>| -> bool {
            match (a, b) {
                (None, None) => true,
                (Some(left), Some(right)) => {
                    crate::database::normalize_text(left) == crate::database::normalize_text(right)
                }
                _ => false,
            }
        };

        // Every lookup below reads the entry and none of them writes to it, so one
        // immutable borrow covers the lot. It used to be lifted into owned
        // `content` / `html_content` first, purely so the borrow could end before
        // `entry_mut.id = id` further down — which copies the whole payload, and
        // for a rich-text capture that payload includes the HTML with its embedded
        // images.
        let image_hash = {
            let entry = ctx.entry.as_ref().expect("discovery produced an entry");
            let content = entry.content.as_str();
            let content_type = entry.content_type.as_str();
            let html_content = entry.html_content.as_deref();

            let normalized_content = crate::database::normalize_text(content);
            let rich_text_html_matches = |id: i64| -> bool {
                if let Ok(Some((_content, c_type, h_content))) = db_state
                    .repo
                    .get_entry_content_with_html_with_conn(&conn, id)
                {
                    if c_type != "rich_text" {
                        return false;
                    }
                    return htmls_equivalent(html_content, h_content.as_deref());
                }
                false
            };

            let types_to_check = if content_type == "rich_text" {
                vec!["rich_text", "text", "code", "url"]
            } else {
                vec![content_type]
            };

            // An image hash comes from the decoded pixels, so every lookup repeats a
            // base64 decode plus a full image decode. Both lookups below run against
            // the same picture — the second differs only by trimmed / CRLF-folded
            // whitespace, which `calc_image_hash` strips anyway — so decode once and
            // hand the result to both. The persistence stage stores the same value
            // as `content_hash`, so it is kept on the context for that stage too.
            let image_hash = if content_type == "image" {
                crate::database::calc_image_hash(content)
            } else {
                None
            };

            for t in types_to_check {
                if let Ok(Some(id)) =
                    db_state
                        .repo
                        .find_by_content_with_hash(&conn, content, Some(t), image_hash)
                {
                    if content_type == "rich_text" && t == "rich_text" && !rich_text_html_matches(id)
                    {
                        continue;
                    }
                    existing_id = Some(id);
                    break;
                }
                if let Ok(Some(id)) = db_state.repo.find_by_content_with_hash(
                    &conn,
                    &normalized_content,
                    Some(t),
                    image_hash,
                ) {
                    if content_type == "rich_text" && t == "rich_text" && !rich_text_html_matches(id)
                    {
                        continue;
                    }
                    existing_id = Some(id);
                    break;
                }
            }
            image_hash
        };
        ctx.image_hash = image_hash;

        if persistent_enabled {
            if let Some(id) = existing_id {
                let entry_mut = ctx.entry.as_mut().unwrap();
                entry_mut.id = id;
            }
        }

        let session_history = ctx.app_handle.state::<SessionHistory>();
        let mut removed_ids = Vec::new();
        let mut reuse_session_id: Option<i64> = None;
        {
            let session = session_history.0.lock().unwrap();
            let entry = ctx.entry.as_ref().expect("entry exists");
            let normalized_entry_content = crate::database::normalize_text(&entry.content);
            for item in session.iter() {
                let html_match = if entry.content_type == "rich_text"
                    && item.content_type == "rich_text"
                {
                    htmls_equivalent(item.html_content.as_deref(), entry.html_content.as_deref())
                } else {
                    true
                };
                // The exact comparison answers almost every item, and only the
                // remainder needs the CRLF-folded form. Normalising first meant
                // copying every remembered item's full content once per capture —
                // with a full 500-item session that is 500 copies of every
                // payload, for a comparison the first term already decides.
                let match_found = (item.content == entry.content
                    || crate::database::normalize_text(&item.content) == normalized_entry_content)
                    && html_match;
                if match_found {
                    removed_ids.push(item.id);
                    if !persistent_enabled {
                        reuse_session_id = Some(item.id);
                    }
                }
            }
        }

        if !persistent_enabled {
            if let Some(reuse_id) = reuse_session_id {
                ctx.reuse_session_id = Some(reuse_id);
                if let Some(entry_mut) = ctx.entry.as_mut() {
                    entry_mut.id = reuse_id;
                }
                removed_ids.retain(|id| *id != reuse_id);
            }
        }

        ctx.pending_removals.extend(removed_ids);
    }
}

impl PipelineStage for ValidationStage {
    fn process(&self, ctx: &mut PipelineContext) {
        if self.should_stop_for_sequential_echo(ctx) {
            println!("Ignoring echo paste from queue");
            ctx.should_stop = true;
            return;
        }

        if let Some(entry) = ctx.entry.as_ref() {
            if is_text_type(&entry.content_type)
                && (entry.content.len()
                    + entry.preview.len()
                    + entry
                        .html_content
                        .as_ref()
                        .map(|html| html.len())
                        .unwrap_or(0))
                    > MAX_PERSISTED_TEXT_BYTES
            {
                println!(
                    "Ignoring oversized clipboard entry: type={}, bytes={}",
                    entry.content_type,
                    entry.content.len()
                        + entry.preview.len()
                        + entry
                            .html_content
                            .as_ref()
                            .map(|html| html.len())
                            .unwrap_or(0)
                );
                ctx.should_stop = true;
                return;
            }
        }

        self.process_deduplication(ctx);
    }
}

// Stage 4: Persistence
/// The row id OCR should run against, or `None` when it must not run at all.
///
/// A failed save leaves the entry at `id = 0`, and OCR is spawned with that id
/// in hand — starting it anyway would attach recognised text to no row and pay
/// for a full decode to find out. Only a newly captured image, with OCR turned
/// on, that actually reached the database qualifies.
fn ocr_target_id(is_new_image: bool, ocr_enabled: bool, saved_id: Option<i64>) -> Option<i64> {
    if is_new_image && ocr_enabled {
        saved_id
    } else {
        None
    }
}

pub struct PersistenceStage;
impl PipelineStage for PersistenceStage {
    fn process(&self, ctx: &mut PipelineContext) {
        let settings = ctx.app_handle.state::<SettingsState>();

        if settings.persistent.load(Ordering::Relaxed) {
            let app_data_dir = ctx.app_handle.state::<AppDataDir>();
            let data_dir = app_data_dir.0.lock().unwrap().clone();
            let db_state = ctx.app_handle.state::<DbState>();

            let (is_new_image, saved_id) = {
                let entry = ctx.entry.as_mut().expect("discovery produced an entry");
                let conn = db_state.conn.lock().unwrap();

                let is_new_image = entry.id == 0 && entry.content_type == "image";
                let saved_id =
                    match db_state.repo.save_with_conn_and_image_hash(
                        &conn,
                        entry,
                        Some(&data_dir),
                        ctx.image_hash,
                    ) {
                        Ok(id) => {
                            entry.id = id;
                            if let Ok(deleted_ids) = db_state
                                .repo
                                .enforce_limit_with_conn(&conn, Some(&data_dir))
                            {
                                for rid in deleted_ids {
                                    let _ = ctx.app_handle.emit("clipboard-removed", rid);
                                }
                            }
                            Some(id)
                        }
                        Err(_) => None,
                    };
                (is_new_image, saved_id)
            };

            // The image payload is needed here and nowhere else: after the write,
            // after the connection is released, and only when OCR is on. Copying it
            // out before the write meant holding a second copy of a screenshot's
            // data URL for the whole of a stage that may never look at it.
            if let Some(id) = ocr_target_id(
                is_new_image,
                settings.ocr_enabled.load(Ordering::Relaxed),
                saved_id,
            ) {
                let content = ctx.entry.as_ref().map(|e| (id, e.content.as_str()));
                if let Some((id, content)) = content {
                    if let Some(png_bytes) =
                        crate::services::clipboard_ops::resolve_image_bytes(content)
                    {
                        let app = ctx.app_handle.clone();
                        tauri::async_runtime::spawn(
                            crate::services::clipboard_ops::trigger_ocr_for_image_item(
                                id, png_bytes, app,
                            ),
                        );
                    }
                }
            }
        } else {
            let entry = ctx.entry.as_mut().expect("discovery produced an entry");
            // Session-only items
            if let Some(reuse_id) = ctx.reuse_session_id {
                let session_history = ctx.app_handle.state::<SessionHistory>();
                let mut updated_entry: Option<ClipboardEntry> = None;
                {
                    let mut session = session_history.0.lock().unwrap();
                    if let Some(existing) = session.iter_mut().find(|i| i.id == reuse_id) {
                        let preserved_tags = existing.tags.clone();
                        let preserved_pinned = existing.is_pinned;
                        let preserved_pinned_order = existing.pinned_order;
                        let preserved_use_count = existing.use_count;

                        existing.content_type = entry.content_type.clone();
                        existing.content = entry.content.clone();
                        existing.html_content = entry.html_content.clone();
                        existing.source_app = entry.source_app.clone();
                        existing.source_app_path = entry.source_app_path.clone();
                        existing.timestamp = entry.timestamp;
                        existing.preview = entry.preview.clone();
                        existing.is_external = entry.is_external;
                        existing.file_preview_exists = entry.file_preview_exists;
                        existing.is_pinned = preserved_pinned;
                        existing.pinned_order = preserved_pinned_order;
                        existing.tags = if entry.tags.is_empty() {
                            preserved_tags
                        } else {
                            entry.tags.clone()
                        };
                        existing.use_count = preserved_use_count + 1;

                        updated_entry = Some(existing.clone());
                    }
                }

                if let Some(updated) = updated_entry {
                    *entry = updated;
                    return;
                }
            }

            // Use a unique negative ID for new session-only items
            let id = -(SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as i64
                / 1000);
            entry.id = id;
            let session_history = ctx.app_handle.state::<SessionHistory>();
            let mut session = session_history.0.lock().unwrap();
            session.push_back(entry.clone());
            if session.len() > 500 {
                if let Some(removed) = session.pop_front() {
                    let _ = ctx.app_handle.emit("clipboard-removed", removed.id);
                }
            }
        }
    }
}

// Stage 5: Distribution
pub struct DistributionStage;
impl PipelineStage for DistributionStage {
    fn process(&self, ctx: &mut PipelineContext) {
        let entry = ctx.entry.as_ref().unwrap();
        let settings = ctx.app_handle.state::<SettingsState>();

        if entry.id == 0 && settings.persistent.load(Ordering::Relaxed) {
            return; // Failed to save
        }

        // The main window is off screen for nearly its whole life, and its
        // WebView2 renderer sits at the low memory target while it is. Pushing a
        // capture into that renderer wakes it up to re-sort and re-render a list
        // nobody is looking at, which is exactly what the hide paid to avoid.
        // Everything the frontend needs is already durable by this point, so the
        // capture is recorded now and the next show hands the window one refresh.
        let deliver = crate::app::idle_destroyer::should_deliver_capture(
            crate::app::idle_destroyer::main_window_on_screen(),
            !IS_DESTROYED.load(Ordering::Relaxed),
        );
        if !deliver {
            crate::app::idle_destroyer::mark_frontend_catchup_pending();
        }

        if !ctx.pending_removals.is_empty() {
            let mut pending = std::mem::take(&mut ctx.pending_removals);
            pending.retain(|id| *id != entry.id);
            if !pending.is_empty() {
                let unique: std::collections::HashSet<i64> = pending.into_iter().collect();
                {
                    let session_history = ctx.app_handle.state::<SessionHistory>();
                    let mut session = session_history.0.lock().unwrap();
                    session.retain(|item| !unique.contains(&item.id));
                }
                if deliver {
                    for rid in unique {
                        let _ = ctx.app_handle.emit("clipboard-removed", rid);
                    }
                }
            }
        }

        // Sequential Queue updates
        if settings.sequential_mode.load(Ordering::Relaxed) {
            let queue_state = ctx.app_handle.state::<PasteQueue>();
            let mut queue = queue_state.0.lock().unwrap();
            if queue.last_action_was_paste {
                queue.items.clear();
                queue.last_action_was_paste = false;
                queue.last_pasted_content = None;
            }
            queue.items.push_back(entry.id);
        }

        // Sound. Deliberately not gated: the point of the setting is to tell the
        // user a capture landed while the window was out of the way, and
        // suppressing it would change a feature the user opted into.
        if settings.sound_enabled.load(Ordering::Relaxed) {
            let _ = ctx.app_handle.emit("play-sound", "copy");
        }

        // Notify
        if deliver {
            let _ = ctx
                .app_handle
                .emit("clipboard-updated", truncate_entry_for_ui(entry));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every payload type discovery has to classify, and the file case in
    // particular: the extension table decides whether one copied path is shown
    // as a picture, a video, or a file, and moving the payload out by value
    // touched every arm of that match.
    #[test]
    fn single_files_are_classified_by_extension() {
        for image in ["a.gif", "a.png", "a.jpg", "a.jpeg", "a.bmp", "a.webp"] {
            assert_eq!(classify_file_path(image), "image", "{}", image);
        }
        for video in [
            "a.mp4", "a.mkv", "a.avi", "a.mov", "a.wmv", "a.flv", "a.webm",
        ] {
            assert_eq!(classify_file_path(video), "video", "{}", video);
        }
        for other in ["a.pdf", "a.txt", "a.zip", "a", "png.png.txt"] {
            assert_eq!(classify_file_path(other), "file", "{}", other);
        }
    }

    #[test]
    fn extension_matching_ignores_case() {
        assert_eq!(classify_file_path("A.PNG"), "image");
        assert_eq!(classify_file_path("A.Mp4"), "video");
    }

    // A save that fails leaves the entry at id 0. OCR is spawned with that id in
    // hand, so letting it run would attach recognised text to no row and pay for
    // a full image decode to find out.
    #[test]
    fn a_failed_save_does_not_start_ocr() {
        assert_eq!(ocr_target_id(true, true, None), None);
    }

    #[test]
    fn ocr_runs_only_for_a_saved_new_image_with_ocr_on() {
        assert_eq!(ocr_target_id(true, true, Some(42)), Some(42));
        assert_eq!(
            ocr_target_id(false, true, Some(42)),
            None,
            "an entry that was already in the database is not new"
        );
        assert_eq!(
            ocr_target_id(true, false, Some(42)),
            None,
            "OCR turned off must not run"
        );
    }

    #[test]
    fn a_non_image_capture_never_reaches_ocr() {
        assert_eq!(ocr_target_id(false, true, Some(7)), None);
        assert_eq!(ocr_target_id(false, false, Some(7)), None);
    }
}
