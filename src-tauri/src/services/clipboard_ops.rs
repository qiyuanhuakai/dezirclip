// Clipboard operations module
use crate::app_state::{SessionHistory, SettingsState};
use crate::database::DbState;
use crate::error::{AppError, AppResult};
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
#[cfg(target_os = "windows")]
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
#[cfg(target_os = "windows")]
use std::sync::OnceLock;
use tauri::{Emitter, Manager, State};
use urlencoding::decode;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::AttachThreadInput;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
};

const RICH_IMAGE_FALLBACK_PREFIX: &str = "<!--TIEZ_RICH_IMAGE:";
const RICH_IMAGE_FALLBACK_SUFFIX: &str = "-->";

fn split_rich_html_and_image_fallback(html: &str) -> (String, Option<String>) {
    if let Some(start) = html.rfind(RICH_IMAGE_FALLBACK_PREFIX) {
        let marker_start = start + RICH_IMAGE_FALLBACK_PREFIX.len();
        if let Some(end_rel) = html[marker_start..].find(RICH_IMAGE_FALLBACK_SUFFIX) {
            let marker_end = marker_start + end_rel;
            let mut cleaned = String::with_capacity(html.len());
            cleaned.push_str(&html[..start]);
            cleaned.push_str(&html[marker_end + RICH_IMAGE_FALLBACK_SUFFIX.len()..]);

            let payload = html[marker_start..marker_end].trim();
            if payload.is_empty() {
                return (cleaned.trim().to_string(), None);
            }
            // Accept both data URL fallback and persisted local file path fallback.
            return (cleaned.trim().to_string(), Some(payload.to_string()));
        }
    }
    (html.to_string(), None)
}

pub fn resolve_image_bytes(content: &str) -> Option<Vec<u8>> {
    resolve_rich_image_fallback_bytes(content)
}

fn resolve_rich_image_fallback_bytes(payload: &str) -> Option<Vec<u8>> {
    let value = payload.trim();

    if value.starts_with("data:image/") {
        let b64_data = value.split(',').nth(1)?;
        if b64_data.is_empty() {
            return None;
        }
        return general_purpose::STANDARD.decode(b64_data).ok();
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
        return None;
    }

    std::fs::read(decoded_path).ok()
}

fn convert_image_content_to_base64(content: &str) -> AppResult<String> {
    if let Some(bytes) = resolve_rich_image_fallback_bytes(content) {
        return Ok(general_purpose::STANDARD.encode(bytes));
    }

    let value = content.trim();
    if value.is_empty() {
        return Err(AppError::Validation(
            "图片内容为空，无法转换为 base64".to_string(),
        ));
    }
    if general_purpose::STANDARD.decode(value).is_ok() {
        return Ok(value.to_string());
    }

    Err(AppError::Validation(
        "无法识别图片内容，无法转换为 base64".to_string(),
    ))
}

pub fn copy_image_bytes_to_system_clipboard(bytes: Vec<u8>) -> AppResult<()> {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    owned_clipboard_writer()?.release_private_selection().map_err(|error|AppError::Internal(error.to_string()))?;
    let hashes = copy_image_bytes_to_clipboard(bytes, current_time)?;
    crate::LAST_APP_SET_HASH.store(hashes.0, Ordering::SeqCst);
    crate::LAST_APP_SET_HASH_ALT.store(hashes.1, Ordering::SeqCst);
    crate::LAST_APP_SET_TIMESTAMP.store(current_time, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn copy_to_clipboard(
    app_handle: tauri::AppHandle,
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    mut content: String,
    mut content_type: String,
    paste: bool,
    id: i64,
    delete_after_use: bool,
    paste_with_format: Option<bool>,
    move_to_top: Option<bool>,
    paste_image_as_base64: Option<bool>,
    expected_privacy_epoch: Option<u64>,
    derived_from_id: Option<i64>,
    expected_host_instance_id:Option<String>,
) -> AppResult<()> {
    if paste && dzc_platform::runtime::is_native_wayland() {
        return Err(AppError::Validation("wayland_interaction_unavailable".into()));
    }
    let bridge=app_handle.state::<crate::app::ipc_host::DesktopHost>();
    let source_id=if id!=0 {id}else{derived_from_id.unwrap_or(0)};
    bridge.check_instance(source_id,expected_host_instance_id.as_deref()).map_err(AppError::Validation)?;
    if bridge.embedded().map_err(AppError::Validation)?.is_none() {
        if paste {handle_window_focus_for_paste(&app_handle).await?;}
        if source_id==0 {
            let upload=bridge.begin_generated_copy(expected_privacy_epoch).await.map_err(AppError::Validation)?;
            let body=generated_copy_body(&content,&content_type,paste_image_as_base64.unwrap_or(false))?;
            upload.finish_copy(body,paste).await.map_err(AppError::Validation)?;
            if paste {play_paste_sound_if_enabled(&app_handle);}
            return Ok(());
        }
        let epoch=expected_privacy_epoch.unwrap_or(state.history_service().runtime_snapshot(Utc::now().timestamp_millis()).map_err(|error|AppError::Validation(error.to_string()))?.privacy_epoch);
        let _:dzc_host::wire::OperationResult=bridge.call(dzc_host::wire::Method::DesktopClipboardCopy,dzc_host::wire::DesktopCopyParams {
            id:source_id,service_instance_id:bridge.instance().map_err(AppError::Validation)?,expected_epoch:epoch,reveal:expected_privacy_epoch.is_some(),paste,
            with_format:paste_with_format,derived_text:derived_from_id.filter(|_|id==0).map(|_|content.clone()),image_as_base64:paste_image_as_base64,
            delete_after_use:Some(delete_after_use),move_to_top,
        },"OperationResult").await.map_err(AppError::Validation)?;
        if paste {play_paste_sound_if_enabled(&app_handle);}
        return Ok(());
    }
    let operation_permission=if let Some(epoch)=expected_privacy_epoch {
        Some(state.history_service().privacy().authorize_read(dzc_core::api::PrivacyAccessIntent::GeneratedCopy,dzc_core::api::SensitiveAccess::Denied,Some(epoch),chrono::Utc::now().timestamp_millis()).map_err(|error|AppError::Validation(error.to_string()))?)
    } else {None};
    let generated_ticket = if paste_image_as_base64.unwrap_or(false) || derived_from_id.is_some() {
        crate::services::clipboard::generated_capture_ticket(&app_handle).await?
    } else { None };

    println!(
        "[DEBUG] copy_to_clipboard called: id={}, paste={}, content_type={}, content_len={}",
        id,
        paste,
        content_type,
        content.len()
    );

    let mut html_content: Option<String> = None;

    let history=state.history_service();
    let mut read_permit=None;
    let mut private_payload=None;
    if id != 0 {
        let sensitive=if expected_privacy_epoch.is_some() { dzc_core::api::SensitiveAccess::ExplicitUserAction } else { dzc_core::api::SensitiveAccess::Denied };
        let permit=history.privacy().authorize_selected_copy(id,expected_host_instance_id.as_deref(),sensitive,expected_privacy_epoch,chrono::Utc::now().timestamp_millis()).map_err(|error|AppError::Validation(error.to_string()))?;
        let entry=if id>0 { history.get_entry_authorized(id,&permit,chrono::Utc::now().timestamp_millis()).map_err(|error|AppError::Validation(error.to_string()))?.ok_or_else(||AppError::Validation("SensitiveAccessDenied".into()))? }
            else {
                let entry=session.0.lock().map_err(|_|AppError::Internal("PrivacyStateUnavailable".into()))?.iter().find(|entry|entry.id==id).cloned().ok_or_else(||AppError::Validation("ClipboardRecordUnavailable".into()))?;
                history.authorize_session_entry_for_host(&entry,&bridge.instance().map_err(AppError::Validation)?,&permit,chrono::Utc::now().timestamp_millis()).map_err(|error|AppError::Validation(error.to_string()))?
            };
        content=entry.content.clone();html_content=entry.html_content.clone();content_type=entry.content_type.clone();
        let payload=if id>0 { history.clipboard_payload_authorized(id,&permit,chrono::Utc::now().timestamp_millis()) } else { history.session_clipboard_payload_for_host_authorized(&entry,&bridge.instance().map_err(AppError::Validation)?,&permit,chrono::Utc::now().timestamp_millis()) }.map_err(|error|AppError::Validation(error.to_string()))?;
        if matches!(payload,dzc_core::ports::ClipboardPayload::Private{..}|dzc_core::ports::ClipboardPayload::ProtectedFiles{..}) { private_payload=Some(payload); }
        read_permit=Some(permit);
    } else if let Some(source_id)=derived_from_id {
        if source_id==0 || !matches!(content_type.as_str(),"text"|"code"|"url"|"rich_text") { return Err(AppError::Validation("DerivedPayloadKindRejected".into())); }
        let sensitive=if expected_privacy_epoch.is_some(){dzc_core::api::SensitiveAccess::ExplicitUserAction}else{dzc_core::api::SensitiveAccess::Denied};
        let permit=history.privacy().authorize_selected_copy(source_id,expected_host_instance_id.as_deref(),sensitive,expected_privacy_epoch,chrono::Utc::now().timestamp_millis()).map_err(|error|AppError::Validation(error.to_string()))?;
        let payload=if source_id>0 {history.derived_text_payload_authorized(source_id,content.clone(),&permit,chrono::Utc::now().timestamp_millis())} else {
            let source=session.0.lock().map_err(|_|AppError::Internal("PrivacyStateUnavailable".into()))?.iter().find(|entry|entry.id==source_id).cloned().ok_or_else(||AppError::Validation("ClipboardRecordUnavailable".into()))?;
            history.session_derived_text_payload_for_host_authorized(&source,&bridge.instance().map_err(AppError::Validation)?,content.clone(),&permit,chrono::Utc::now().timestamp_millis())
        }.map_err(|error|AppError::Validation(error.to_string()))?;
        if matches!(payload,dzc_core::ports::ClipboardPayload::Private{..}){ private_payload=Some(payload); }
        read_permit=Some(permit);
    }

    let paste_image_as_base64_requested = paste_image_as_base64.unwrap_or(false);
    let mut effective_content_type = content_type.clone();
    let mut self_insert_label: Option<String> = None;

    if content_type == "image" && paste_image_as_base64_requested {
        if id != 0 {
            // Image-to-Base64 conversion on an existing image entry. Decode the image
            // payload and re-encode as a plain base64 string so the clipboard monitor
            // and downstream consumers see it as TEXT.
            content = convert_image_content_to_base64(&content)?;
            effective_content_type = "text".to_string();
            self_insert_label = Some("DezirClip Image to Base64".to_string());
        } else {
            // Defensive: id=0 with content_type="image" + paste_image_as_base64 may be
            // sent by an OCR-paste caller where the payload is already plain text. Skip
            // the image decoder (it would fail on OCR text) and plan the explicit insert.
            effective_content_type = "text".to_string();
            self_insert_label = Some("DezirClip OCR Paste".to_string());
        }
    }

    if derived_from_id.is_some() && private_payload.is_some() && self_insert_label.is_none() { self_insert_label=Some("DezirClip Protected Derived".into()); }

    // 1. Handle Window Visibility and Focus
    if paste {
        handle_window_focus_for_paste(&app_handle).await?;
    }

    // 2. For image-to-Base64 and OCR-paste paths, synchronously insert the resulting
    // TEXT entry via the clipboard pipeline BEFORE writing to the system clipboard.
    // The pipeline assigns a fresh timestamp strictly greater than the source entry's,
    // guaranteeing the new TEXT row lands at the top of history. The subsequent
    // clipboard write below is then marked as a self-copy so the OS clipboard monitor
    // does not race us by creating a duplicate row.
    if let Some(label) = self_insert_label.as_ref() {
        crate::services::clipboard::process_new_entry_with_protection(
            &app_handle,
            crate::services::clipboard::ClipboardData::Text(content.clone()),
            Some(label.clone()),
            None,
            generated_ticket,
            private_payload.is_some(),
        ).await?;
    }

    // 3. Copy to system clipboard. For self-insert paths we force mark_as_self_copy=true
    // to suppress the monitor; otherwise we delegate to the standard decision helper.
    let mark_as_self_copy = if self_insert_label.is_some() {
        true
    } else {
        should_mark_clipboard_write_as_self_copy(id, paste, paste_image_as_base64_requested)
    };

    if let Some(permit)=&operation_permission { history.privacy().check_read(permit,chrono::Utc::now().timestamp_millis()).map_err(|error|AppError::Validation(error.to_string()))?; }
    if let Some(permit)=&read_permit { history.privacy().check_read(permit,chrono::Utc::now().timestamp_millis()).map_err(|error|AppError::Validation(error.to_string()))?; }
    if let Some(mut payload)=private_payload {
        if let dzc_core::ports::ClipboardPayload::Private{payload:inner,..}=&mut payload {
            if self_insert_label.is_some() || !paste_with_format.unwrap_or(effective_content_type=="rich_text" && html_content.is_some()) && matches!(inner.as_ref(),dzc_core::ports::ClipboardPayload::Html{..}) { **inner=dzc_core::ports::ClipboardPayload::Text(content.clone()); }
        }
        bridge.writer().map_err(AppError::Validation)?.write_suppressing_recapture(payload).map_err(|error|AppError::Internal(error.to_string()))?;
    } else {
    write_content_to_system_clipboard(
        &content,
        ClipboardWriteOptions {
            content_type: &effective_content_type,
            html_content: html_content.as_deref(),
            paste_with_format: paste_with_format.unwrap_or(
                effective_content_type == "rich_text" && html_content.as_deref().is_some(),
            ),
            mark_as_self_copy,
        },
    )?;
    }

    // 4. Perform paste action if requested
    if paste {
        perform_paste_action(
            &app_handle,
            &state,
            id,
            delete_after_use,
            Some(&content),
            &effective_content_type,
            move_to_top,
        )
        .await?;
    }

    Ok(())
}

fn generated_copy_body(content:&str,kind:&str,image_as_text:bool)->AppResult<dzc_host::wire::GeneratedBody> {
    let (kind,text)=if kind=="image" && !image_as_text {
        let unavailable=||AppError::Validation("GeneratedImageUnavailable".into());
        let maximum=16*1024*1024;
        let value=content.trim();
        let bytes=if value.starts_with("data:image/") {
            if value.len()>maximum*4/3+1024 {return Err(unavailable());}
            let (header,data)=value.split_once(',').ok_or_else(unavailable)?;
            if !header.ends_with(";base64") {return Err(unavailable());}
            general_purpose::STANDARD.decode(data).map_err(|_|unavailable())?
        } else {
            let raw=value.strip_prefix("file://").unwrap_or(value);
            let raw=if raw.starts_with('/') && raw.chars().nth(2)==Some(':') {&raw[1..]}else{raw};
            let path=decode(raw).map_err(|_|unavailable())?;
            dzc_core::filesystem::read_bounded_regular(std::path::Path::new(path.as_ref()),maximum).map_err(|_|unavailable())?
        };
        if bytes.len()>maximum {return Err(unavailable());}
        let format=image::guess_format(&bytes).map_err(|_|unavailable())?;
        ("image",format!("data:{};base64,{}",format.to_mime_type(),general_purpose::STANDARD.encode(bytes)))
    } else if matches!(kind,"text"|"code"|"url"|"rich_text"|"image") {("text",content.into())}
    else {return Err(AppError::Validation("GeneratedPayloadKindRejected".into()));};
    Ok(dzc_host::wire::GeneratedBody{kind:kind.into(),text,html:None})
}

async fn handle_window_focus_for_paste(app_handle: &tauri::AppHandle) -> AppResult<()> {
    let quick_paste_was_focused = hide_quick_paste_before_paste(app_handle).await;

    // 1. Only restore focus if our window actually took focus; avoids unnecessary focus flips
    // that can force fullscreen apps into windowed mode.
    if !quick_paste_was_focused && crate::IS_MAIN_WINDOW_FOCUSED.load(Ordering::Relaxed) {
        let _ = restore_focus_before_paste(app_handle).await;
    }

    // 2. Then handle the specific visibility logic based on pinned state
    if crate::WINDOW_PINNED.load(Ordering::Relaxed) {
        // In pinned mode, stay visible but ensure window does NOT have focus
        if let Some(window) = app_handle.get_webview_window("main") {
            // Make sure the window doesn't steal focus back
            let _ = window.set_focusable(false);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    } else {
        // In auto-hide mode, hide the window now
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.hide();
            crate::app::webview_memory::lower_window_memory(&window, "paste-hide");
            // IS_HIDDEN only means "hidden by edge docking"; paste hides should clear it.
            crate::IS_HIDDEN.store(false, std::sync::atomic::Ordering::Relaxed);
            crate::app::window_manager::release_win_keys();
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    Ok(())
}

async fn hide_quick_paste_before_paste(app_handle: &tauri::AppHandle) -> bool {
    let Some(window) = app_handle.get_webview_window("quick-paste") else {
        return false;
    };
    if !window.is_visible().unwrap_or(false) {
        return false;
    }

    let _ = window.set_focusable(false);
    let _ = window.hide();
    let _ = restore_focus_before_paste(app_handle).await;
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    true
}

async fn restore_focus_before_paste(_app_handle: &tauri::AppHandle) -> AppResult<()> {
    let last_hwnd_val = crate::LAST_ACTIVE_HWND.load(Ordering::Relaxed);
    if last_hwnd_val == 0 {
        return Err(AppError::Internal(
            "No last active window captured".to_string(),
        ));
    }

    #[cfg(target_os = "windows")]
    {
        let target_hwnd = HWND(last_hwnd_val as _);
        unsafe {
            if !IsWindowVisible(target_hwnd).as_bool() {
                return Err(AppError::Internal(
                    "Target window is no longer visible".to_string(),
                ));
            }

            let fg_hwnd = GetForegroundWindow();
            if fg_hwnd.0 != target_hwnd.0 {
                let fg_thread_id = GetWindowThreadProcessId(fg_hwnd, None);
                let target_thread_id = GetWindowThreadProcessId(target_hwnd, None);

                if fg_thread_id != 0 && target_thread_id != 0 && fg_thread_id != target_thread_id {
                    let _ = AttachThreadInput(fg_thread_id, target_thread_id, true);
                    let _ = SetForegroundWindow(target_hwnd);
                    if IsIconic(target_hwnd).as_bool() {
                        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                            target_hwnd,
                            windows::Win32::UI::WindowsAndMessaging::SW_RESTORE,
                        );
                    }
                    let _ = windows::Win32::UI::WindowsAndMessaging::BringWindowToTop(target_hwnd);
                    let _ = AttachThreadInput(fg_thread_id, target_thread_id, false);
                } else {
                    let _ = SetForegroundWindow(target_hwnd);
                    if IsIconic(target_hwnd).as_bool() {
                        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                            target_hwnd,
                            windows::Win32::UI::WindowsAndMessaging::SW_RESTORE,
                        );
                    }
                    let _ = windows::Win32::UI::WindowsAndMessaging::BringWindowToTop(target_hwnd);
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let _ =
            crate::infrastructure::linux_api::window_tracker::activate_window_focus(last_hwnd_val);
    }

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    Ok(())
}

fn calculate_content_hash(content: &str) -> (u64, u64) {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let content_hash = hasher.finish();

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    (content_hash, current_time)
}

pub(crate) struct ClipboardWriteOptions<'a> {
    pub content_type: &'a str,
    pub html_content: Option<&'a str>,
    pub paste_with_format: bool,
    pub mark_as_self_copy: bool,
}

pub(crate) fn should_mark_clipboard_write_as_self_copy(
    id: i64,
    paste: bool,
    paste_image_as_base64: bool,
) -> bool {
    paste || (id != 0 && !paste_image_as_base64)
}

pub(crate) fn write_content_to_system_clipboard(
    content: &str,
    options: ClipboardWriteOptions<'_>,
) -> AppResult<()> {
    let (content_hash, current_time) = calculate_content_hash(content);

    let clipboard_hashes = if cfg!(target_os="linux") {
        write_native_wayland_content(content, &options)?;
        (content_hash.max(1), 0)
    } else {
        owned_clipboard_writer()?
            .release_private_selection().map_err(|error| AppError::Internal(error.to_string()))?;
        match options.content_type {
        "image" | "video" | "file" => {
            copy_file_like_content(content, options.content_type, current_time, content_hash)?
        }
        ct if ct == "rich_text" || (options.paste_with_format && options.html_content.is_some()) => {
            copy_rich_text_content(
                content,
                options.html_content,
                options.paste_with_format,
                current_time,
            )?
        }
        _ => {
            copy_text_with_retry(content)?;
            (content_hash, 0)
        }
    }};

    if options.mark_as_self_copy {
        crate::LAST_APP_SET_HASH.store(clipboard_hashes.0, Ordering::SeqCst);
        crate::LAST_APP_SET_HASH_ALT.store(clipboard_hashes.1, Ordering::SeqCst);
        crate::LAST_APP_SET_TIMESTAMP.store(current_time, Ordering::SeqCst);
    }

    Ok(())
}

fn write_native_wayland_content(content: &str, options: &ClipboardWriteOptions<'_>) -> AppResult<()> {
    use dzc_core::ports::{ClipboardPayload as Payload, ClipboardWriter};
    let payload = match options.content_type {
        "image" => {
            const LIMIT: usize = 64 * 1024 * 1024;
            let bytes = if content.starts_with('/') || content.starts_with("file://") || content.contains(":\\") {
                let path = crate::infrastructure::attachments::path_from_uri(content).map_err(AppError::Internal)?;
                dzc_platform::local_file::read_bounded_regular(std::path::Path::new(&path), LIMIT).map_err(AppError::from)?
            } else {
                let encoded = content.split_once(',').filter(|_| content.starts_with("data:image/")).map(|(_, data)| data).unwrap_or(content);
                if encoded.len() > (LIMIT / 3 + 1) * 4 { return Err(AppError::Validation("WaylandTransferBudgetExceeded".into())); }
                general_purpose::STANDARD.decode(encoded).map_err(|_| AppError::Validation("ClipboardImageInvalid".into()))?
            };
            let mime = match image::guess_format(&bytes).map_err(|_| AppError::Validation("ClipboardImageInvalid".into()))? {
                image::ImageFormat::Png => "image/png",
                image::ImageFormat::Jpeg => "image/jpeg",
                image::ImageFormat::Gif => "image/gif",
                image::ImageFormat::WebP => "image/webp",
                image::ImageFormat::Bmp => "image/bmp",
                _ => return Err(AppError::Validation("ClipboardImageInvalid".into())),
            };
            Payload::Image { bytes, mime: mime.into() }
        }
        "file" | "files" | "video" if content.starts_with('/') || content.contains(":\\") => Payload::Files(split_local_paths(content)),
        _ if options.paste_with_format && options.html_content.is_some() => {
            let (html, _) = split_rich_html_and_image_fallback(options.html_content.expect("checked above"));
            Payload::Html { html, text: content.to_string() }
        }
        _ => Payload::Text(content.to_string()),
    };
    let writer = owned_clipboard_writer()?;
    if options.mark_as_self_copy { writer.write_suppressing_recapture(payload) } else { writer.write(payload) }
        .map_err(|error| AppError::Internal(error.to_string()))
}

fn owned_clipboard_writer()->AppResult<dzc_platform::runtime::EmbeddedClipboardWriter> {
    if let Some(app)=crate::global_state::GLOBAL_APP_HANDLE.get() {
        return app.state::<crate::app::ipc_host::DesktopHost>().writer().map_err(AppError::Validation);
    }
    #[cfg(test)]
    return Ok(dzc_platform::runtime::shared_embedded_clipboard_writer().clone());
    #[cfg(not(test))]
    Err(AppError::Validation("ClipboardHostUnavailable".into()))
}

fn copy_file_like_content(
    content: &str,
    content_type: &str,
    current_time: u64,
    content_hash: u64,
) -> AppResult<(u64, u64)> {
    if !content.starts_with("data:") && (content.starts_with('/') || content.contains(":\\")) {
        return copy_local_path_content(content, content_type, current_time);
    }

    if content_type == "image" {
        return copy_inline_image_content(content, current_time);
    }

    copy_text_with_retry(content)?;
    Ok((content_hash.max(1), 0))
}

fn copy_local_path_content(
    content: &str,
    content_type: &str,
    current_time: u64,
) -> AppResult<(u64, u64)> {
    let paths = split_local_paths(content);

    if content_type == "image" {
        let Some(path) = paths.first() else {
            return Err(AppError::Validation(
                "图片文件路径为空，无法复制到剪贴板".to_string(),
            ));
        };

        let bytes = std::fs::read(path).map_err(AppError::from)?;
        return copy_image_bytes_to_clipboard(bytes, current_time);
    }

    #[cfg(target_os = "windows")]
    {
        unsafe {
            crate::infrastructure::windows_api::win_clipboard::set_clipboard_files(vec![
                content.to_string()
            ])
            .map_err(AppError::from)?;
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::infrastructure::linux_api::clipboard::set_clipboard_files(paths)
            .map_err(AppError::from)?;
    }

    let (path_hash, _) = calculate_content_hash(content);
    Ok((path_hash.max(1), 0))
}

fn copy_inline_image_content(content: &str, current_time: u64) -> AppResult<(u64, u64)> {
    let b64_data = if content.starts_with("data:image") {
        content.split(',').nth(1).unwrap_or(content)
    } else {
        content
    };

    let bytes = general_purpose::STANDARD
        .decode(b64_data)
        .map_err(|e| AppError::Internal(format!("Base64 解码失败: {}", e)))?;

    copy_image_bytes_to_clipboard(bytes, current_time)
}

fn copy_rich_text_content(
    content: &str,
    html_content: Option<&str>,
    paste_with_format: bool,
    _current_time: u64,
) -> AppResult<(u64, u64)> {
    let Some(html) = html_content else {
        copy_text_with_retry(content)?;
        let (content_hash, _) = calculate_content_hash(content);
        return Ok((content_hash.max(1), 0));
    };

    #[cfg(target_os = "linux")]
    if dzc_platform::runtime::is_native_wayland() && paste_with_format {
        let (html, _) = split_rich_html_and_image_fallback(html);
        crate::infrastructure::linux_api::clipboard::set_clipboard_text_and_html(content.to_string(), Some(html))?;
        let (content_hash, _) = calculate_content_hash(content);
        return Ok((content_hash.max(1), 0));
    }

    if !paste_with_format {
        copy_text_with_retry(content)?;
        let (content_hash, _) = calculate_content_hash(content);
        return Ok((content_hash.max(1), 0));
    }

    let (_clean_html, fallback_image_data_url) = split_rich_html_and_image_fallback(html);
    #[cfg(target_os = "windows")]
    let html_for_paste = if _clean_html.trim().is_empty() {
        html
    } else {
        _clean_html.as_str()
    };
    #[cfg(target_os = "windows")]
    let cf_html = generate_cf_html(html_for_paste);

    if let Some(payload) = fallback_image_data_url {
        if let Some(bytes) = resolve_rich_image_fallback_bytes(&payload) {
            #[cfg(target_os = "windows")]
            let image_hashes = copy_image_bytes_to_clipboard(bytes, _current_time)?;
            #[cfg(not(target_os = "windows"))]
            copy_image_bytes_to_clipboard(bytes, _current_time)?;
            let (content_hash, _) = calculate_content_hash(content);
            #[cfg(target_os = "windows")]
            {
                unsafe {
                    crate::infrastructure::windows_api::win_clipboard::append_clipboard_text_and_html(content, &cf_html)
                        .map_err(AppError::from)?;
                }
                return Ok((content_hash.max(1), image_hashes.0.max(image_hashes.1)));
            }
            #[cfg(not(target_os = "windows"))]
            {
                crate::infrastructure::linux_api::clipboard::set_clipboard_text_and_html(
                    content.to_string(),
                    None,
                )?;
                return Ok((content_hash.max(1), 0));
            }
        }
    }

    #[cfg(target_os = "windows")]
    unsafe {
        crate::infrastructure::windows_api::win_clipboard::set_clipboard_text_and_html(
            content, &cf_html,
        )
        .map_err(AppError::from)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::infrastructure::linux_api::clipboard::set_clipboard_text_and_html(
            content.to_string(),
            None,
        )?;
    }
    let (content_hash, _) = calculate_content_hash(content);
    Ok((content_hash.max(1), 0))
}

#[cfg(target_os = "windows")]
fn generate_cf_html(html: &str) -> String {
    static BODY_OPEN_RE: OnceLock<Regex> = OnceLock::new();
    static BODY_CLOSE_RE: OnceLock<Regex> = OnceLock::new();
    static HTML_TAG_RE: OnceLock<Regex> = OnceLock::new();

    let body_open_re = BODY_OPEN_RE.get_or_init(|| Regex::new(r"(?is)<body\b[^>]*>").unwrap());
    let body_close_re = BODY_CLOSE_RE.get_or_init(|| Regex::new(r"(?is)</body\s*>").unwrap());
    let html_tag_re = HTML_TAG_RE.get_or_init(|| Regex::new(r"(?is)<html\b").unwrap());

    let wrap_with_body = |fragment: &str| {
        format!(
            "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n</head>\n<body>\n<!--StartFragment-->{}<!--EndFragment-->\n</body>\n</html>",
            fragment
        )
    };

    let mut html_content = html.to_string();
    let has_html_tag = html_tag_re.is_match(&html_content);
    let has_start = html_content.contains("<!--StartFragment-->");
    let has_end = html_content.contains("<!--EndFragment-->");

    if !has_html_tag {
        html_content = wrap_with_body(&html_content);
    } else if !(has_start && has_end) {
        if let Some(open_match) = body_open_re.find(&html_content) {
            let open_end = open_match.end();

            if !has_end {
                if let Some(close_match) = body_close_re.find(&html_content) {
                    if close_match.start() >= open_end {
                        html_content.insert_str(close_match.start(), "<!--EndFragment-->");
                    } else {
                        html_content.push_str("<!--EndFragment-->");
                    }
                } else {
                    html_content.push_str("<!--EndFragment-->");
                }
            }

            if !has_start {
                html_content.insert_str(open_end, "<!--StartFragment-->");
            }
        } else {
            html_content = wrap_with_body(&html_content);
        }
    }

    if !(html_content.contains("<!--StartFragment-->")
        && html_content.contains("<!--EndFragment-->"))
    {
        html_content = wrap_with_body(&html_content);
    }

    let header_placeholder = format!(
        "Version:0.9\r\nStartHTML:{:0>10}\r\nEndHTML:{:0>10}\r\nStartFragment:{:0>10}\r\nEndFragment:{:0>10}\r\n",
        0,
        0,
        0,
        0
    );
    let start_html = header_placeholder.len();
    let start_fragment = start_html
        + html_content.find("<!--StartFragment-->").unwrap()
        + "<!--StartFragment-->".len();
    let end_fragment = start_html + html_content.find("<!--EndFragment-->").unwrap();
    let end_html = start_html + html_content.len();

    let header = format!(
        "Version:0.9\r\nStartHTML:{:0>10}\r\nEndHTML:{:0>10}\r\nStartFragment:{:0>10}\r\nEndFragment:{:0>10}\r\n",
        start_html,
        end_html,
        start_fragment,
        end_fragment
    );
    format!("{}{}", header, html_content)
}
fn copy_image_bytes_to_clipboard(bytes: Vec<u8>, _current_time: u64) -> AppResult<(u64, u64)> {
    // Check if it's a GIF by magic number
    let is_gif = bytes.len() > 3 && &bytes[0..3] == b"GIF";

    // One decode feeds the pixel hash, the clipboard payload and the PNG export.
    // This used to decode a second time just to re-encode, which doubled peak
    // memory on large screenshots.
    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| AppError::Internal(format!("加载图像失败: {}", e)))?;
    // Already-RGBA8 sources (what the clipboard hands us) move instead of being
    // copied by `to_rgba8`.
    let rgba = match decoded {
        image::DynamicImage::ImageRgba8(buffer) => buffer,
        other => other.to_rgba8(),
    };
    let (width, height) = rgba.dimensions();

    let (primary_hash, secondary_hash) = if is_gif {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        let byte_hash = hasher.finish();

        // Calculate pixel hash of the first frame as a secondary fingerprint
        let pixels = rgba.as_raw();
        let pixel_count = (width as u64) * (height as u64);
        let mut h = pixel_count;
        if !pixels.is_empty() {
            h = h
                .wrapping_add(pixels[0] as u64)
                .wrapping_add(pixels[pixels.len() / 2] as u64)
                .wrapping_add(pixels[pixels.len() - 1] as u64);
        }
        (byte_hash, h)
    } else {
        // Hash full pixel bytes so the monitor can skip our own image copy
        let mut hasher = DefaultHasher::new();
        rgba.as_raw().as_slice().hash(&mut hasher);
        let byte_hash = hasher.finish();
        (byte_hash, 0)
    };

    #[cfg(target_os = "windows")]
    {
        // Prepare PNG data for better compatibility. Sources that already are a
        // PNG go through untouched instead of paying for another encode.
        let png_buf: Vec<u8> = if crate::services::image_png::is_png(&bytes) {
            bytes.clone()
        } else {
            let mut encoded: Vec<u8> = Vec::new();
            // `write_image` takes the pixel buffer by reference, so encoding the
            // non-PNG case does not need its own copy of the pixels.
            image::ImageEncoder::write_image(
                image::codecs::png::PngEncoder::new(&mut encoded),
                rgba.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| AppError::Internal(format!("编码 PNG 失败: {}", e)))?;
            encoded
        };

        let gif_temp_path = unsafe {
            crate::infrastructure::windows_api::win_clipboard::set_clipboard_image_with_formats(
                crate::infrastructure::windows_api::win_clipboard::ImageData {
                    width: width as usize,
                    height: height as usize,
                    bytes: rgba.into_raw(),
                },
                if is_gif { Some(&bytes) } else { None },
                Some(&png_buf),
            )
            .map_err(AppError::from)?
        };

        if let Some(path) = gif_temp_path {
            let mut hasher = DefaultHasher::new();
            path.hash(&mut hasher);
            let path_hash = hasher.finish();
            crate::LAST_APP_SET_HASH.store(path_hash, Ordering::SeqCst);
        }
    }

    #[cfg(target_os = "linux")]
    {
        use dzc_core::ports::ClipboardWriter;
        owned_clipboard_writer()?.write_suppressing_recapture(dzc_core::ports::ClipboardPayload::Image{bytes,mime:if is_gif{"image/gif"}else{"image/png"}.into()})
            .map_err(|error|AppError::Internal(error.to_string()))?;
    }

    Ok((primary_hash, secondary_hash))
}

fn copy_text_with_retry(content: &str) -> AppResult<()> {
    println!("[DEBUG] Copying text to clipboard: {} chars", content.len());
    #[cfg(target_os = "linux")]
    {
        crate::infrastructure::linux_api::clipboard::set_clipboard_text_and_html(
            content.to_string(),
            None,
        )?;
        println!("[DEBUG] Text copied to clipboard successfully");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let mut retries = 3;
        while retries > 0 {
            let res = {
                let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
                clipboard.set_text(content.to_string())
            };

            match res {
                Ok(_) => {
                    println!("[DEBUG] Text copied to clipboard successfully");
                    return Ok(());
                }
                Err(_e) if retries > 1 => {
                    retries -= 1;
                    println!(
                        "[DEBUG] Clipboard set failed, retrying... ({} left)",
                        retries
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(AppError::Internal(format!("Clipboard error: {}", e))),
            }
        }
        Ok(())
    }
}

async fn perform_paste_action(
    app_handle: &tauri::AppHandle,
    state: &State<'_, DbState>,
    id: i64,
    delete_after_use: bool,
    content: Option<&str>,
    content_type: &str,
    move_to_top: Option<bool>,
) -> AppResult<()> {
    println!(
        "[DEBUG] perform_paste_action: pinned={}",
        crate::WINDOW_PINNED.load(Ordering::Relaxed)
    );
    wait_for_focus_settle().await;
    recover_focus_if_stolen(app_handle).await;
    let paste_method = resolve_paste_method(state);
    send_paste_keystroke(&paste_method, content, Some(content_type))?;
    hide_window_after_paste(app_handle).await;
    handle_post_paste_actions(app_handle, state, id, delete_after_use, move_to_top)?;
    play_paste_sound_if_enabled(app_handle);

    Ok(())
}

async fn wait_for_focus_settle() {
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
}

#[cfg(target_os = "windows")]
fn is_main_window_foreground(app_handle: &tauri::AppHandle) -> bool {
    unsafe {
        let foreground = GetForegroundWindow();
        if let Some(window) = app_handle.get_webview_window("main") {
            if let Ok(hwnd_raw) = window.hwnd() {
                return foreground.0 == hwnd_raw.0;
            }
        }
    }
    false
}

#[cfg(not(target_os = "windows"))]
fn is_main_window_foreground(_app_handle: &tauri::AppHandle) -> bool {
    false
}

async fn recover_focus_if_stolen(app_handle: &tauri::AppHandle) {
    if is_main_window_foreground(app_handle) {
        println!("[WARN] Clipboard window STOLE focus back, attempting one last restore...");
        let _ = restore_focus_before_paste(app_handle).await;
    }
}

fn resolve_paste_method(state: &State<'_, DbState>) -> String {
    state
        .settings_repo
        .get("app.paste_method")
        .ok()
        .flatten()
        .unwrap_or_else(|| "shift_insert".to_string())
}

async fn hide_window_after_paste(app_handle: &tauri::AppHandle) {
    if crate::WINDOW_PINNED.load(Ordering::Relaxed) {
        // In pinned mode, keep window non-focusable and restore focus back to last app
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.set_focusable(false);
        }
        let _ = restore_focus_before_paste(app_handle).await;
        return;
    }

    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.set_focusable(false);
        let _ = window.hide();
        crate::app::webview_memory::lower_window_memory(&window, "paste-hide-after");
        // IS_HIDDEN only means "hidden by edge docking"; paste hides should clear it.
        crate::IS_HIDDEN.store(false, std::sync::atomic::Ordering::Relaxed);
        crate::NAVIGATION_ENABLED.store(false, Ordering::Relaxed); // Disable navigation like hide_window_cmd does
        crate::app::window_manager::release_win_keys();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

pub fn send_paste_keystroke(
    method: &str,
    content: Option<&str>,
    content_type: Option<&str>,
) -> AppResult<()> {
    println!("[DEBUG] Sending paste keystroke using method: {}", method);
    #[cfg(target_os = "windows")]
    crate::infrastructure::windows_api::paste::send_paste(method, content, content_type)
        .map_err(AppError::Internal)?;

    #[cfg(target_os = "linux")]
    {
        crate::infrastructure::linux_api::paste::simulate_paste_with_method_and_content(
            method,
            content_type,
            content,
        )
            .map_err(AppError::from)?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        std::process::Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to keystroke \"v\" using command down",
            ])
            .spawn()
            .ok();
    }

    Ok(())
}

fn split_local_paths(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect()
}

fn handle_post_paste_actions(
    app_handle: &tauri::AppHandle,
    state: &State<'_, DbState>,
    id: i64,
    delete_after_use: bool,
    move_to_top: Option<bool>,
) -> AppResult<()> {
    if delete_after_use {
        // Cleanup file if needed
        let app_data = app_handle.state::<crate::app_state::AppDataDir>();
        let data_dir = app_data.0.lock().unwrap();

        if state.repo.delete(id, Some(&data_dir)).is_ok() {
            let _ = app_handle.emit("clipboard-removed", id);
        }
    } else if id > 0 {
        let _ = state.repo.increment_use_count(id);

        let should_move_to_top = match move_to_top {
            Some(val) => val,
            None => state
                .settings_repo
                .get("app.move_to_top_after_paste")
                .ok()
                .flatten()
                .map(|v| v != "false")
                .unwrap_or(true),
        };

        if should_move_to_top {
            let should_promote = state
                .repo
                .get_entry_by_id(id)
                .ok()
                .flatten()
                .map(|entry| !entry.is_pinned)
                .unwrap_or(true);
            if should_promote {
                let _ = state.repo.touch_entry(id, Utc::now().timestamp_millis());
            }
        }
    }

    Ok(())
}

fn play_paste_sound_if_enabled(app_handle: &tauri::AppHandle) {
    let settings = app_handle.state::<SettingsState>();
    if settings.sound_enabled.load(Ordering::Relaxed) {
        let _ = app_handle.emit("play-sound", "paste");
    }
}

#[tauri::command]
pub fn paste_latest_rich(app_handle: tauri::AppHandle) {
    let app_handle_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let delete_after = {
            let settings = app_handle_clone.state::<SettingsState>();
            settings.delete_after_paste.load(Ordering::Relaxed)
        };

        let history = crate::app::commands::history_cmd::get_clipboard_history(
            app_handle_clone.clone(),
            app_handle_clone.state::<DbState>(),
            app_handle_clone.state::<SessionHistory>(),
            1,
            0, // offset
            None,
        )
        .await;

        if let Ok(items) = history {
            if let Some(item) = items.first() {
                let _ = copy_to_clipboard(
                    app_handle_clone.clone(),
                    app_handle_clone.state::<DbState>(),
                    app_handle_clone.state::<SessionHistory>(),
                    item.content.clone(),
                    item.content_type.clone(),
                    true, // paste
                    item.id,
                    delete_after, // delete_after_use
                    Some(true),   // paste_with_format
                    None,
                    None,
                    None,
                    None,
                    item.host_instance_id.clone(),
                )
                .await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_remote_copy_normalizes_favorite_images_and_keeps_ocr_as_text() {
        let emoji=generated_copy_body("🎸","text",false).unwrap();
        assert_eq!((emoji.kind.as_str(),emoji.text.as_str()),("text","🎸"));
        let image="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVQI12P4z8AAAAMBAQAY3Y2wAAAAAElFTkSuQmCC";
        let directory=std::env::temp_dir().join(format!("dzc-generated-image-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path=directory.join("favorite.png");
        std::fs::write(&path,general_purpose::STANDARD.decode(image.split_once(',').unwrap().1).unwrap()).unwrap();
        let favorite=generated_copy_body(path.to_str().unwrap(),"image",false).unwrap();
        assert_eq!(favorite.kind,"image");
        assert_eq!(favorite.text,image);
        assert!(favorite.html.is_none());
        let ocr=generated_copy_body("ordinary OCR result","image",true).unwrap();
        assert_eq!((ocr.kind.as_str(),ocr.text.as_str()),("text","ordinary OCR result"));
        assert!(generated_copy_body("data:image/png,not-base64","image",false).is_err());
        assert!(generated_copy_body("/synthetic/file","files",false).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_wayland_gui_write_intent_preserves_generated_capture() {
        if std::env::var("DEZIRCLIP_RUN_WAYLAND_SMOKE_TESTS").as_deref() != Ok("1") { return; }
        use dzc_core::capture::*;
        use dzc_core::api::{HistoryQuery, HistoryService};
        use std::{collections::VecDeque, sync::Mutex};
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(runtime.join("dezirclip-isolated-wayland-fixture").is_file());
        assert!(std::env::var_os("DISPLAY").is_none());
        let history = HistoryService::open(&runtime.join("gui-write-intent.db")).unwrap();
        let store = history.capture_store(&runtime);
        let session = dzc_core::api::SessionHistoryStore::new(VecDeque::new());
        let context = CaptureContext { source: CaptureSource::default(), timestamp: 12345,
            settings: CaptureSettings { persistent: true, ..Default::default() }, session: &session, paused: false, sequential_echo: None };
        for (id, text) in [(0, "synthetic conversion text"), (0, "synthetic OCR text"), (7, "synthetic existing history copy")] {
            write_content_to_system_clipboard(text, ClipboardWriteOptions { content_type: "text", html_content: None,
                paste_with_format: false, mark_as_self_copy: should_mark_clipboard_write_as_self_copy(id, false, false) }).unwrap();
            let result = CaptureEngine::capture(&context, &mut dzc_platform::capture::NativeCaptureReader::default(), &store, &mut ());
            if id == 0 { assert!(matches!(result, CaptureOutcome::Stored(_))); }
            else { assert!(matches!(result, CaptureOutcome::Skipped(CaptureSkip::OwnClipboard))); }
        }
        let rows = history.query(&HistoryQuery { limit: 10, ..Default::default() }).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.content == "synthetic conversion text"));
        assert!(rows.iter().any(|row| row.content == "synthetic OCR text"));
    }

    // `copy_image_bytes_to_clipboard` hashes the decoded pixels so the monitor
    // can recognise and skip our own copy. The monitor side hashes the
    // `Vec<u8>` it reads back off the clipboard; the copy side hashes a slice of
    // the decoded buffer. Those two must keep producing the same digest or the
    // app starts re-capturing every image it puts on the clipboard.
    #[test]
    fn pixel_hash_matches_between_vec_and_slice() {
        let pixels: Vec<u8> = vec![7, 11, 13, 17, 19, 23, 29, 31];

        let mut from_vec = DefaultHasher::new();
        pixels.hash(&mut from_vec);

        let mut from_slice = DefaultHasher::new();
        pixels.as_slice().hash(&mut from_slice);

        assert_eq!(from_vec.finish(), from_slice.finish());
    }

    #[test]
    fn self_copy_hash_preserves_whitespace_and_line_endings() {
        let hash = |s: &str| calculate_content_hash(s).0;

        let lf = "hello\nworld";
        let crlf = "hello\r\nworld";
        assert_ne!(hash(crlf), hash(lf));
        assert_ne!(hash("  padded  "), hash("padded"));

        let mixed = "a\r\nb\nc";
        let mut hasher = DefaultHasher::new();
        mixed.hash(&mut hasher);
        assert_eq!(hash(mixed), hasher.finish());

        assert_ne!(hash("alpha"), hash("beta"), "different text must not collide");
    }


    #[test]
    fn test_generated_clipboard_writes_are_not_marked_as_self_copy() {
        assert!(
            should_mark_clipboard_write_as_self_copy(7, true, false),
            "pasting an existing entry must suppress re-capture"
        );
        assert!(
            should_mark_clipboard_write_as_self_copy(7, false, false),
            "copying an existing entry must suppress duplicate history rows"
        );
        assert!(
            !should_mark_clipboard_write_as_self_copy(0, false, false),
            "generated text such as transforms must be visible to the clipboard monitor"
        );
        assert!(
            !should_mark_clipboard_write_as_self_copy(7, false, true),
            "image-to-base64 creates new text and must be visible to the clipboard monitor"
        );
    }

    #[test]
    fn test_should_mark_clipboard_write_as_self_copy_all_eight_combos() {
        // Truth table for should_mark_clipboard_write_as_self_copy: returns true iff
        // the helper would mark the write as a self-copy. Formula:
        //   paste || (id != 0 && !paste_image_as_base64)
        // Note: copy_to_clipboard additionally overrides mark_as_self_copy=true for
        // the image-to-base64 case (id != 0, paste_image_as_base64=true) so the OS
        // clipboard monitor does not race the explicit history insert; that override
        // lives at the call site and is not visible in the helper-function result.
        let cases: &[(i64, bool, bool, bool)] = &[
            (0, false, false, false),
            (0, false, true, false),
            (0, true, false, true),
            (0, true, true, true),
            (7, false, false, true),
            (7, false, true, false),
            (7, true, false, true),
            (7, true, true, true),
        ];
        assert_eq!(cases.len(), 8, "must cover all 2x2x2 combinations");
        for (id, paste, paste_image_as_base64, expected) in cases {
            let actual =
                should_mark_clipboard_write_as_self_copy(*id, *paste, *paste_image_as_base64);
            assert_eq!(
                actual, *expected,
                "should_mark_clipboard_write_as_self_copy(id={}, paste={}, paste_image_as_base64={}) expected {}, got {}",
                id, paste, paste_image_as_base64, expected, actual
            );
        }
    }

    #[test]
    fn test_convert_image_content_to_base64_data_url_round_trip() {
        let bytes = [0u8, 1, 2, 3, 0xFF, 0x80, 0x40, 0x20];
        let b64 = general_purpose::STANDARD.encode(bytes);
        let data_url = format!("data:image/png;base64,{}", b64);
        let result = convert_image_content_to_base64(&data_url);
        let decoded = result.expect("data url must decode");
        assert_eq!(decoded, b64);
    }

    #[test]
    fn test_convert_image_content_to_base64_already_base64_passthrough() {
        let b64 = general_purpose::STANDARD.encode([9u8, 8, 7, 6]);
        let result = convert_image_content_to_base64(&b64);
        let decoded = result.expect("raw base64 must pass through");
        assert_eq!(decoded, b64);
    }

    #[test]
    fn test_convert_image_content_to_base64_empty_errors() {
        let result = convert_image_content_to_base64("");
        assert!(result.is_err(), "empty content must error");
    }

    #[test]
    fn test_convert_image_content_to_base64_unrecognized_errors() {
        let result = convert_image_content_to_base64("not base64 nor data url @@@");
        assert!(result.is_err(), "unrecognized content must error");
    }
}
