use crate::{Host, Session, router::{now_ms, privacy_error}};
use dzc_core::{api::{PrivacyAccessIntent, SensitiveAccess}, ports::ClipboardPayload,
    capture::{CaptureData, CaptureFrame, CaptureReadMetadata, CaptureSource}};
use dzc_protocol::wire::*;

fn local_files(paths: Vec<String>) -> Result<Vec<String>, WireErrorCode> {
    if paths.is_empty() || paths.len() > 256 || paths.iter().map(String::len).sum::<usize>() > 512 * 1024 {
        return Err(WireErrorCode::InvalidRequest);
    }
    for path in &paths {
        let file = std::path::Path::new(path);
        if path.contains(['\0', '\n', '\r']) || !file.is_absolute() || !file.is_file() {
            return Err(WireErrorCode::InvalidRequest);
        }
    }
    Ok(paths)
}

impl Host {
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
                        metadata: CaptureReadMetadata { files: true, privacy_ticket: Some(ticket), ..Default::default() } },
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
        let paths = match body {
            ClipboardPayload::Files(paths) => local_files(paths.clone())?,
            ClipboardPayload::Image { .. } | ClipboardPayload::ProtectedFiles { .. } => {
                let export = if let Some(row) = &row {
                    self.history.session_external_files_for_host_authorized(row, &request.service_instance_id, &permit, now)
                } else { self.history.external_files_authorized(request.id, &permit, now) }.map_err(privacy_error)?;
                let lease = dzc_platform::runtime::private_attachment_export(export).map_err(|_| WireErrorCode::Unavailable)?;
                let paths = lease.paths().map_err(|_| WireErrorCode::Unavailable)?;
                self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
                lease.retain();
                paths
            }
            _ => return Err(WireErrorCode::InvalidRequest),
        };
        self.history.privacy().check_read(&permit, now_ms()?).map_err(privacy_error)?;
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        Ok(FileDragResult { paths })
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
            assert_eq!(*writer.0.lock().unwrap(), vec![paths]);
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
        let host = crate::router::tests::synthetic_host();
        let state = host.snapshot().unwrap();
        for client in [session(&host, ClientRole::Desktop, false), session(&host, ClientRole::Cli, true)] {
            for (method, params) in [
                (Method::DesktopFilesDrop, serde_json::json!({"paths":["/missing"],"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch})),
                (Method::DesktopFilesDrag, serde_json::json!({"id":-1,"reveal":false,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch})),
            ] {
                let result = host.dispatch(&client, Request { request_id: "file-authority".into(), method, params });
                assert_eq!(result.error.unwrap().code, WireErrorCode::PermissionDenied);
            }
        }
        let desktop = session(&host, ClientRole::Desktop, true);
        for instance in [state.service_instance_id.clone(), "replaced-host".into()] {
            let result = host.drag_files(&desktop, FileDragParams { id: -1, reveal: false,
                service_instance_id: instance, expected_epoch: state.privacy_epoch + 1 });
            assert!(result.is_err(), "stale identity must never export files");
        }
    }

    #[test]
    fn native_drag_resolves_real_session_files_without_deleting_them() {
        let host = crate::router::tests::synthetic_host();
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
        assert_eq!(result.paths, paths);
        assert_eq!(std::fs::read(&file).unwrap(), b"native drag fixture");
        host.session.lock().unwrap()[0].tags.push("sensitive".into());
        assert!(host.drag_files(&desktop, request()).is_err(), "hidden sensitive files must not escape through drag");
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn file_drop_rejects_empty_relative_and_missing_paths() {
        for paths in [vec![], vec!["relative.txt".into()], vec!["/dzc-missing-file\nother".into()],
            vec!["/dzc-nonexistent-dropped-file".into()], vec!["x".into(); 257]] {
            assert_eq!(local_files(paths).unwrap_err(), WireErrorCode::InvalidRequest);
        }
    }
}
