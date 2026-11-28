use crate::{Host, Session, router::{now_ms, privacy_error}};
use dzc_core::{api::{PrivacyAccessIntent, SensitiveAccess}, ports::{AuthorizedFileExport, ClipboardPayload, PlatformResult},
    capture::{CaptureData, CaptureFrame, CaptureReadMetadata, CaptureSource}};
use dzc_platform::runtime::PrivateFileExport;
use dzc_protocol::wire::*;

fn local_files(paths: Vec<String>) -> Result<Vec<String>, WireErrorCode> {
    if paths.is_empty() || paths.len() > 256 || paths.iter().map(String::len).sum::<usize>() > 512 * 1024 {
        return Err(WireErrorCode::InvalidRequest);
    }
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for path in paths {
        let file = std::path::Path::new(&path);
        if path.contains(['\0', '\n', '\r']) || !file.is_absolute() || !file.is_file() {
            return Err(WireErrorCode::InvalidRequest);
        }
        let canonical = dunce::canonicalize(file).map_err(|_| WireErrorCode::InvalidRequest)?;
        let value = canonical.to_str().ok_or(WireErrorCode::InvalidRequest)?.to_owned();
        let key = if cfg!(target_os = "windows") { value.to_lowercase() } else { value.clone() };
        if seen.insert(key) { unique.push(value); }
    }
    Ok(unique)
}

impl Host {
    pub(crate) fn share_history_item(&self, session: &Session, request: ShareEntryParams) -> Result<bool, WireErrorCode> {
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        return Err(WireErrorCode::UnsupportedCapability);
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            if request.id == 0 {
                return Err(WireErrorCode::InvalidRequest);
            }
            let now = now_ms()?;
            let privacy = self.history.privacy();
            let access = if request.reveal { SensitiveAccess::ExplicitUserAction } else { SensitiveAccess::Denied };
            let permit = privacy.authorize_read(PrivacyAccessIntent::InteractiveRead, access,
                Some(request.expected_epoch), now).map_err(privacy_error)?;
            let row = self.entry_authorized(request.id, &permit, now)?;
            let authorized = || !self.is_stopping() && self.grants.still_valid(session)
                && now_ms().is_ok_and(|now| privacy.check_read(&permit, now).is_ok());
            let attachments = || FileDragParams {
                    id: request.id,
                    service_instance_id: request.service_instance_id.clone(),
                    expected_epoch: request.expected_epoch,
                    reveal: request.reveal,
            };
            #[cfg(target_os = "windows")]
            let opened = {
                use dzc_platform::apps::WindowsShare;
                let contents = match row.content_type.as_str() {
                    "text" | "code" | "url" | "link" | "rich_text" => WindowsShare::Text {
                        text: row.content, html: row.html_content,
                        link: matches!(row.content_type.as_str(), "url" | "link"),
                    },
                    "image" | "file" | "files" | "video" => WindowsShare::Files {
                        paths: self.drag_files(session, attachments())?.paths,
                        image: row.content_type == "image",
                    },
                    _ => return Err(WireErrorCode::UnsupportedCapability),
                };
                dzc_platform::apps::show_windows_share(contents, authorized)
            };
            #[cfg(target_os = "linux")]
            let opened = {
                let (body, files, lease) = match row.content_type.as_str() {
                    "text" | "code" | "url" | "link" | "rich_text" => (Some(row.content), Vec::new(), None),
                    "image" | "file" | "files" | "video" => {
                        let (paths, lease) = self.prepare_file_transfer(
                            session, &attachments(), dzc_platform::runtime::private_email_attachment_export,
                        )?;
                        (None, paths, lease)
                    }
                    _ => return Err(WireErrorCode::UnsupportedCapability),
                };
                let opened = dzc_platform::apps::compose_share_email(body, files, || {
                    authorized() && lease.as_ref().is_none_or(PrivateFileExport::is_current)
                });
                if matches!(opened, Ok(true)) {
                    if let Some(lease) = lease { lease.retain(); }
                }
                opened
            };
            opened.map_err(|error| match error.kind() {
                std::io::ErrorKind::PermissionDenied => WireErrorCode::PrivacyBlocked,
                std::io::ErrorKind::WouldBlock => WireErrorCode::Busy,
                _ => WireErrorCode::Unavailable,
            })
        }
    }

    pub(crate) fn drop_files(&self, session: &Session, request: FileDropParams) -> Result<(), WireErrorCode> {
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        session.require(Permission::ClipboardCopy)?;
        if !self.actions.can_copy() { return Err(WireErrorCode::UnsupportedCapability); }
        let paths = local_files(request.paths)?;
        let now = now_ms()?;
        let privacy = self.history.privacy();
        let permit = privacy.authorize_read(PrivacyAccessIntent::GeneratedCopy, SensitiveAccess::Denied,
            Some(request.expected_epoch), now).map_err(privacy_error)?;
        let payload = self.history.dropped_files_payload_authorized(paths.clone(), &permit, now).map_err(privacy_error)?;
        if self.capture_owner.load(std::sync::atomic::Ordering::Acquire)
            && self.history.capture_settings_snapshot().map_err(|_| WireErrorCode::Unavailable)?.capture_files {
            match privacy.capture_ticket(now) {
                Ok(ticket) => {
                    self.capture_generated(CaptureFrame { data: CaptureData::Files(paths),
                        metadata: CaptureReadMetadata { files: true, deduplicate_generated: true, privacy_ticket: Some(ticket), ..Default::default() } },
                        CaptureSource { app_name: "DezirClip File Drop".into(), process_path: None }, now)?;
                }
                Err(dzc_core::api::PrivacyError::PrivacySessionActive) => {},
                Err(error) => return Err(privacy_error(error)),
            }
        }
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        self.copy_payload(payload)
    }

    // Only the authenticated native Desktop consumes these paths. They are
    // never returned by the WebView binding or made available to Surface/MCP.
    pub(crate) fn drag_files(&self, session: &Session, request: FileDragParams) -> Result<FileDragResult, WireErrorCode> {
        let (paths, lease) = self.prepare_file_transfer(
            session, &request, dzc_platform::runtime::private_attachment_export,
        )?;
        if let Some(lease) = lease { lease.retain(); }
        Ok(FileDragResult { paths })
    }

    fn prepare_file_transfer(
        &self,
        session: &Session,
        request: &FileDragParams,
        export_files: impl FnOnce(AuthorizedFileExport) -> PlatformResult<PrivateFileExport>,
    ) -> Result<(Vec<String>, Option<PrivateFileExport>), WireErrorCode> {
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        session.require(Permission::ClipboardCopy)?;
        if request.id == 0 { return Err(WireErrorCode::InvalidRequest); }
        let now = now_ms()?;
        let permit = self.history.privacy().authorize_selected_copy(request.id, Some(&request.service_instance_id),
            if request.reveal { SensitiveAccess::ExplicitUserAction } else { SensitiveAccess::Denied },
            Some(request.expected_epoch), now).map_err(privacy_error)?;
        let row = if request.id < 0 {
            Some(self.session.lock().map_err(|_| WireErrorCode::Unavailable)?.iter()
                .find(|row| row.id == request.id).cloned().ok_or(WireErrorCode::NotFound)?)
        } else { None };
        let payload = if let Some(row) = &row {
            self.history.session_clipboard_payload_for_host_authorized(row, &request.service_instance_id, &permit, now)
        } else { self.history.clipboard_payload_authorized(request.id, &permit, now) }.map_err(privacy_error)?;
        let mut body = &payload;
        while let ClipboardPayload::Private { payload, .. } = body { body = payload; }
        let result = match body {
            ClipboardPayload::Files(paths) => (local_files(paths.clone())?, None),
            ClipboardPayload::Image { .. } | ClipboardPayload::ProtectedFiles { .. } => {
                let export = if let Some(row) = &row {
                    self.history.session_external_files_for_host_authorized(row, &request.service_instance_id, &permit, now)
                } else { self.history.external_files_authorized(request.id, &permit, now) }.map_err(privacy_error)?;
                let lease = export_files(export).map_err(|_| WireErrorCode::Unavailable)?;
                let paths = lease.paths().map_err(|_| WireErrorCode::Unavailable)?;
                (paths, Some(lease))
            }
            _ => return Err(WireErrorCode::InvalidRequest),
        };
        self.history.privacy().check_read(&permit, now_ms()?).map_err(privacy_error)?;
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct FilesWriter(std::sync::Mutex<Vec<Vec<String>>>);
    impl crate::HostActions for FilesWriter {
        fn platform_capabilities(&self) -> serde_json::Value { serde_json::json!({}) }
        fn can_copy(&self) -> bool { true }
        fn copy(&self, payload: ClipboardPayload) -> Result<(), WireErrorCode> {
            let ClipboardPayload::Private { payload, authorization } = payload else { panic!("file copy must be revocable") };
            assert!(authorization.is_current() && authorization.native_lease().unwrap().is_current());
            let ClipboardPayload::Files(paths) = *payload else { panic!("original files required") };
            self.0.lock().unwrap().push(paths);
            Ok(())
        }
        fn can_paste(&self) -> bool { false }
        fn paste(&self) -> Result<(), WireErrorCode> { panic!("file transfer never injects paste") }
        fn stop_helpers(&self) -> Result<(), WireErrorCode> { Ok(()) }
        fn revoke_private_exports(&self) -> Result<(), WireErrorCode> { Ok(()) }
    }

    #[test]
    fn drop_copies_originals_without_starting_capture_or_paste() {
        use std::sync::Arc;
        let root = std::env::temp_dir().join(format!("dzc-file-copy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let file = root.join("source.txt");
        std::fs::write(&file, b"original").unwrap();
        {
            let writer = Arc::new(FilesWriter::default());
            let host = Host::new(crate::HostOptions { mode: HostMode::Service,
                history: dzc_core::api::HistoryService::open(&root.join("history.db")).unwrap(),
                directory: root.clone(), actions: writer.clone() }).unwrap();
            let client = session(&host, ClientRole::Desktop, true);
            let state = host.snapshot().unwrap();
            let paths = vec![file.to_string_lossy().into_owned()];
            let call = || host.dispatch(&client, Request { request_id: "drop-copy".into(), method: Method::DesktopFilesDrop,
                params: serde_json::json!({"paths":paths,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch}) });
            assert!(call().error.is_none());
            assert_eq!(*writer.0.lock().unwrap(), vec![vec![dunce::canonicalize(&file).unwrap().to_string_lossy().into_owned()]]);
            assert!(host.session.lock().unwrap().is_empty());
            assert_eq!(std::fs::read(&file).unwrap(), b"original");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    fn session(host: &Host, role: ClientRole, control: bool) -> Session {
        let grant = control.then(|| host.grants().issue_control(role).unwrap());
        host.hello(HelloParams { protocol_version: VERSION, client_version: "fixture".into(),
            client_role: role, capabilities: vec![], required_capabilities: vec![],
            grant: grant.map(|grant| grant.token().into()) }).unwrap().0
    }

    #[test]
    fn native_files_require_desktop_authority_and_current_state() {
        let host = crate::fixtures::synthetic_host();
        let state = host.snapshot().unwrap();
        for client in [session(&host, ClientRole::Desktop, false), session(&host, ClientRole::Cli, true)] {
            for (method, params) in [
                (Method::DesktopFilesDrop, serde_json::json!({"paths":["/missing"],"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch})),
                (Method::DesktopFilesDrag, serde_json::json!({"id":-1,"reveal":false,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch})),
                (Method::DesktopHistoryShare, serde_json::json!({"id":-1,"reveal":false,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch})),
            ] {
                let result = host.dispatch(&client, Request { request_id: "file-authority".into(), method, params });
                assert_eq!(result.error.unwrap().code, WireErrorCode::PermissionDenied);
            }
        }
        let desktop = session(&host, ClientRole::Desktop, true);
        for instance in [state.service_instance_id.clone(), "replaced-host".into()] {
            let result = host.drag_files(&desktop, FileDragParams { id: -1, reveal: false,
                service_instance_id: instance.clone(), expected_epoch: state.privacy_epoch + 1 });
            assert!(result.is_err(), "stale identity must never export files");
            assert!(host.share_history_item(&desktop, ShareEntryParams { id: -1, reveal: false,
                service_instance_id: instance, expected_epoch: state.privacy_epoch + 1 }).is_err(),
                "stale identity must never open a share");
        }
    }

    #[test]
    fn native_drag_resolves_real_session_files_without_deleting_them() {
        let host = crate::fixtures::synthetic_host();
        let directory = std::env::temp_dir().join(format!("dzc-drag-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("文件 with spaces.txt");
        std::fs::write(&file, b"native drag fixture").unwrap();
        let paths = vec![file.to_string_lossy().into_owned()];
        let row = serde_json::from_value(serde_json::json!({"id":-1,"content_type":"files",
            "content":serde_json::to_string(&paths).unwrap(),"preview":"file fixture","source_app":"Fixture",
            "timestamp":1,"is_pinned":false,"tags":[]})).unwrap();
        host.session.lock().unwrap().push_back(row);
        let state = host.snapshot().unwrap();
        let desktop = session(&host, ClientRole::Desktop, true);
        let request = || FileDragParams { id: -1, reveal: false, service_instance_id: state.service_instance_id.clone(), expected_epoch: state.privacy_epoch };
        let result = host.drag_files(&desktop, request()).unwrap();
        assert_eq!(result.paths, vec![dunce::canonicalize(&file).unwrap().to_string_lossy().into_owned()]);
        assert_eq!(std::fs::read(&file).unwrap(), b"native drag fixture");
        host.session.lock().unwrap()[0].tags.push("sensitive".into());
        assert!(host.drag_files(&desktop, request()).is_err(), "hidden sensitive files must not escape through drag");
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn email_uses_named_exports_and_requires_reveal_for_protected_files() {
        let host = crate::fixtures::synthetic_host();
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+X2fUAAAAASUVORK5CYII=";
        let archive = r#"dzc-files-v1:{"version":1,"files":[{"name":"附件.txt","data":"cHJvdGVjdGVk"}]}"#;
        for (id, kind, content, tags) in [(-1, "image", png, vec![]), (-2, "files", archive, vec!["sensitive"])] {
            host.session.lock().unwrap().push_back(serde_json::from_value(serde_json::json!({
                "id":id, "content_type":kind, "content":content, "preview":"fixture",
                "source_app":"Fixture", "timestamp":1, "is_pinned":false, "tags":tags,
            })).unwrap());
        }
        let state = host.snapshot().unwrap();
        let desktop = session(&host, ClientRole::Desktop, true);
        let export = |id, reveal| {
            host.prepare_file_transfer(&desktop, &FileDragParams {
                id, reveal, service_instance_id: state.service_instance_id.clone(), expected_epoch: state.privacy_epoch,
            }, dzc_platform::runtime::private_email_attachment_export)
        };
        assert!(matches!(export(-2, false), Err(WireErrorCode::SensitiveAccessDenied)));
        for (id, reveal, name, prefix) in [
            (-1, false, "DezirClip.png", b"\x89PNG".as_slice()),
            (-2, true, "附件.txt", b"protected".as_slice()),
        ] {
            let (paths, lease) = export(id, reveal).unwrap();
            let path = std::path::Path::new(&paths[0]);
            assert_eq!(path.file_name().unwrap(), name);
            assert!(std::fs::read(path).unwrap().starts_with(prefix));
            drop(lease.unwrap());
            assert!(!path.exists());
        }
    }

    #[test]
    fn file_drop_rejects_empty_relative_and_missing_paths() {
        for paths in [vec![], vec!["relative.txt".into()], vec!["/dzc-missing-file\nother".into()],
            vec!["/dzc-nonexistent-dropped-file".into()], vec!["x".into(); 257]] {
            assert_eq!(local_files(paths).unwrap_err(), WireErrorCode::InvalidRequest);
        }
    }
    #[test]
    fn drop_removes_path_aliases_and_preserves_first_attachment_order() {
        let root = std::env::temp_dir().join(format!("dzc-drop-dedup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let alias = root.join(".").join("b.txt");
        let result = local_files(vec![b.to_string_lossy().into(), a.to_string_lossy().into(), alias.to_string_lossy().into()]).unwrap();
        assert_eq!(result, vec![dunce::canonicalize(&b).unwrap().to_string_lossy(), dunce::canonicalize(&a).unwrap().to_string_lossy()]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
