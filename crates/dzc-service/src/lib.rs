//! Optional per-user service. Linking this library starts no workers.
mod diagnostics;
use clap::Parser;
use dzc_host::{CaptureManager, ControlFile, Host, HostOptions, HostActions, wire::{HostMode, WireErrorCode}};
use dzc_core::{api::HistoryService, ports::{PasteController, ClipboardWriter, ClipboardPayload}};
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(name="dzc-service",version,about="Explicit current-user DezirClip Host; never auto-installs a daemon")]
struct Options {
    /// Use a synthetic/private profile or an explicit existing history file.
    #[arg(long)] database: Option<PathBuf>,
    /// Serve history without starting a native clipboard listener.
    #[arg(long)] no_capture: bool,
    /// Query this executable's actual identity without opening a database.
    #[arg(long)] build_info: bool,
}

#[cfg(target_os="windows")]
struct WindowsPreparedPaste(dzc_platform::runtime::WindowsBoundPaste);
#[cfg(target_os="windows")]
impl dzc_host::PreparedPaste for WindowsPreparedPaste {
    fn send(&self)->Result<(),WireErrorCode> {self.0.send().map_err(|_|WireErrorCode::Unavailable)}
}
struct NativeActions { writer: dzc_platform::runtime::EmbeddedClipboardWriter }
impl HostActions for NativeActions {
    fn platform_capabilities(&self) -> serde_json::Value { serde_json::to_value(dzc_platform::runtime::standalone_capabilities()).unwrap_or_default() }
    fn can_copy(&self) -> bool { self.writer.capability().is_available() }
    fn copy(&self, payload: ClipboardPayload) -> Result<(), WireErrorCode> { self.writer.write_suppressing_recapture(payload).map_err(|_| WireErrorCode::Unavailable) }
    fn can_paste(&self) -> bool { dzc_platform::runtime::NativePasteController.capability().is_available() }
    fn paste(&self) -> Result<(), WireErrorCode> { dzc_platform::runtime::NativePasteController.paste().map_err(|_| WireErrorCode::Unavailable) }
    fn supports_windows_bound_paste(&self)->bool {cfg!(target_os="windows")}
    #[cfg(target_os="windows")]
    fn prepare_windows_paste(&self,target:&dzc_host::wire::WindowsPasteTarget)->Result<Box<dyn dzc_host::PreparedPaste>,WireErrorCode> {
        Ok(Box::new(WindowsPreparedPaste(dzc_platform::runtime::WindowsBoundPaste::prepare(target).map_err(|_|WireErrorCode::Unavailable)?)))
    }
    fn stop_helpers(&self) -> Result<(), WireErrorCode> { self.writer.stop().map_err(|_| WireErrorCode::Busy) }
}

pub fn main_entry() -> Result<(), String> {
    dzc_platform::video_preview::dispatch();
    dzc_platform::dispatch_export_guardian();
    let options = Options::parse();
    if options.build_info {
        println!("{}", serde_json::to_string(&dzc_protocol::build_info::BuildInfo::current(dzc_protocol::build_info::Component::Service, dzc_core::api::DATABASE_SCHEMA_VERSION)).map_err(|_| "BuildInfoUnavailable")?);
        return Ok(());
    }
    dzc_platform::ipc::ProcessExit::ensure_supported().map_err(|_| "ServiceProcessObservationUnsupported")?;
    if let Err(error) = dzc_platform::process_memory::background_service() {
        eprintln!("Service memory priority unavailable: {error}");
    }
    let database = options.database.map(Ok).unwrap_or_else(dzc_host::data::database_path)?;
    let directory = dzc_host::data::canonical_directory(&database)?;
    let diagnostics = diagnostics::Diagnostics::open(&directory);
    diagnostics.install_panic_hook();
    dzc_platform::install_default_key_provider();
    dzc_platform::video_preview::install();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "HostRuntimeUnavailable")?;
    let result = runtime.block_on(async {
        let endpoint = dzc_platform::ipc::Endpoint::for_current_session(&directory).map_err(|_| "HostEndpointUnavailable")?;
        // A migration may own the target endpoint before it publishes clipboard.db.
        // A competing startup must not create or initialize that database.
        let listener = endpoint.bind().map_err(|_| "HostAlreadyRunning")?;
        if dzc_platform::runtime::start_window_target_tracking().is_err() {
            eprintln!("Desktop focus tracking unavailable; direct foreground paste remains available");
        }
        let history = HistoryService::open(&database).map_err(|_| "HostDatabaseUnavailable")?;
        let host = Host::new(HostOptions { mode: HostMode::Service, history, directory: directory.clone(), actions: Arc::new(NativeActions { writer: Default::default() }) }).map_err(|_| "HostInitializationFailed")?;
        let _credentials = ControlFile::publish(&host, &endpoint).map_err(|_| "HostCredentialsUnavailable")?;
        if options.no_capture {host.set_capture_diagnostic("disabled_by_request").map_err(|_|"HostInitializationFailed")?;}
        else {
            let (manager, ready) = CaptureManager::start(&host).map_err(|_| "HostCaptureOwnershipUnavailable")?;
            if ready.is_err() {
                if manager.stop().is_err() { return Err("HostCaptureStopUnconfirmed".to_string()); }
                return Err("HostCaptureUnavailable".to_string());
            }
            if host.initialize_privacy_shortcut().is_err() {
                host.shutdown().map_err(|_|"HostCaptureStopUnconfirmed".to_owned())?;
                return Err("PrivacyShortcutUnavailable".into());
            }
        }
        let (shutdown, receiver) = tokio::sync::watch::channel(false);
        let signal_host = Arc::downgrade(&host);
        let signal_diagnostics = diagnostics.clone();
        ctrlc::set_handler(move || {
            signal_diagnostics.record("shutdown signal");
            if let Some(host) = signal_host.upgrade() {
                if host.shutdown().is_ok() { let _ = shutdown.send(true); }
            }
        }).map_err(|_| "HostSignalUnavailable")?;
        let result = dzc_host::serve_listener(host.clone(), listener, receiver).await;
        diagnostics.record(&format!("listener ended requested={} error_kind={:?} os_error={:?}", host.is_stopping(), result.as_ref().err().map(std::io::Error::kind), result.as_ref().err().and_then(std::io::Error::raw_os_error)));
        result.map_err(|_| "HostTransportFailed".to_string())
    });
    diagnostics.record(match &result { Ok(()) => "exit success", Err(error) => error });
    result
}


#[cfg(test)]
mod desktop_protocol_tests {
    use dzc_host::{controller, wire::*, Client};
    use std::{error::Error, path::{Path, PathBuf}};

    #[test]
    #[ignore = "requires DZC_TEST_SERVICE pointing to the matching prebuilt dzc-service"]
    fn installed_service_serves_desktop_tools_without_a_webview() -> Result<(), Box<dyn Error>> {
        let executable = PathBuf::from(std::env::var_os("DZC_TEST_SERVICE").ok_or("missing service fixture")?);
        let scratch = std::env::temp_dir().join(format!("dzc-desktop-test-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()));
        std::fs::create_dir(&scratch)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700))?;
        }
        let database = scratch.join("history.db");
        let result = tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
            let capture = cfg!(target_os = "linux") && std::env::var_os("DZC_TEST_XVFB_SCREEN").is_some()
                && std::env::var_os("DZC_TEST_XVFB_CAPTURE").is_some();
            controller::start(&database, &executable, !capture).await?;
            let checks = exercise_tools(&database, capture).await;
            controller::stop(&database).await?;
            checks
        });
        std::fs::remove_dir_all(scratch)?;
        result
    }

    async fn exercise_themes(client: &mut Client, database: &Path, state: &RuntimeState) -> Result<(), Box<dyn Error>> {
        use serde_json::json;
        let profile = database.parent().ok_or("missing profile")?;
        let source = profile.join("source.theme");
        let target = profile.join("export.theme");
        let sample = json!({"id":"sample","name":"Sample","base":"mica","format":"dezirclip-theme","version":1,
            "css":":root.light-mode{--dc-accent:#123456}.dark-mode .dc-tag-entry{border-radius:12px}"});
        std::fs::write(&source, serde_json::to_vec(&sample)?)?;
        let base = json!({"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch});
        let mut import = base.clone(); import["path"] = json!(source);
        let saved: DesktopThemePackage = client.call(Method::DesktopThemesImport, &import, "DesktopThemePackage").await?;
        assert_eq!(saved.id, "sample");
        assert!(client.call::<DesktopThemePackage>(Method::DesktopThemesImport, import, "DesktopThemePackage").await.is_err());
        let list: ThemeIndex = client.call(Method::DesktopThemesList, &base, "ThemeIndex").await?;
        assert_eq!(list.ids, ["sample"]);
        let mut selected = base.clone(); selected["id"] = json!("sample");
        let read: DesktopThemePackage = client.call(Method::DesktopThemesGet, &selected, "DesktopThemePackage").await?;
        assert_eq!(read.css, saved.css);
        let mut export = selected.clone(); export["path"] = json!(database);
        assert!(client.call::<OperationResult>(Method::DesktopThemesExport, &export, "OperationResult").await.is_err());
        export["path"] = json!(target);
        let wrote: OperationResult = client.call(Method::DesktopThemesExport, export, "OperationResult").await?;
        assert!(wrote.performed);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(target)?)?, sample);
        let removed: ThemeRemoved = client.call(Method::DesktopThemesRemove, selected, "ThemeRemoved").await?;
        assert!(!removed.active_reset);
        let list: ThemeIndex = client.call(Method::DesktopThemesList, base, "ThemeIndex").await?;
        assert!(list.ids.is_empty());
        Ok(())
    }

    async fn exercise_tools(database: &Path, capture: bool) -> Result<(), Box<dyn Error>> {
        let mut client = Client::control(&controller::endpoint(database)?, ClientRole::Desktop).await?;
        let state: ServiceStatus = client.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
        if state.mode != HostMode::Service || state.capture_owner != capture { return Err("expected the requested isolated service mode".into()); }
        exercise_themes(&mut client, database, &state.state).await?;
        let catalog: ToolCatalog = client.call(Method::DesktopToolsCatalog, serde_json::json!({}), "ToolCatalog").await?;
        let classified: ToolClassification = client.call(Method::DesktopToolsClassify,
            serde_json::json!({"text":"https://example.org"}), "ToolClassification").await?;
        let transformed: ToolText = client.call(Method::DesktopToolsTransform,
            serde_json::json!({"text":"aB", "kind":"to_uppercase"}), "ToolText").await?;
        let qr: ToolText = client.call(Method::DesktopToolsQr,
            serde_json::json!({"content":"本地测试","format":"svg"}), "ToolText").await?;
        if catalog.transforms.len() != 17 || classified.kinds != ["url"] || transformed.text != "AB" || !qr.text.contains("<svg") {
            return Err("standalone tool result mismatch".into());
        }
        exercise_settings(&mut client, &state.state).await?;
        exercise_backups(&mut client, database, &state.state).await?;
        exercise_manual_ocr(&mut client, database, &state.state).await?;
        exercise_library(&mut client, database, &state.state).await?;
        #[cfg(target_os = "linux")]
        if std::env::var_os("DZC_TEST_XVFB_SCREEN").is_some() {
            if let Some(target) = std::env::var_os("DZC_TEST_X11_QUEUE_TARGET") {
                exercise_queue(&mut client, &state.state, Path::new(&target), capture).await?;
                let endpoint = controller::endpoint(database)?;
                let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/tests/native-quick-focus-x11.py");
                let status = std::process::Command::new("python3").arg(script).arg(endpoint.address())
                    .arg(endpoint.control_path()).arg(client.peer.pid().ok_or("missing service PID")?.to_string())
                    .arg(database.parent().ok_or("missing fixture profile")?).arg(target).status()?;
                if !status.success() {return Err("independent desktop focus restoration failed".into());}
            }
            exercise_screen(database, &mut client, &state.state, capture).await?;
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn exercise_screen(database: &Path, client: &mut Client, state: &RuntimeState, capture: bool) -> Result<(), Box<dyn Error>> {
        let base = serde_json::json!({"service_instance_id":state.service_instance_id, "expected_epoch":state.privacy_epoch});
        let start: ScreenImage = client.call(Method::DesktopScreenBegin, &base, "ScreenImage").await?;
        if (start.width, start.height) != (640, 480) { return Err("expected the isolated 640x480 Xvfb".into()); }
        let with_token = |token: &str| {
            let mut p = base.clone();
            p["token"] = token.into();
            p
        };
        let mut read = with_token(&start.token);
        read["offset"] = 0.into();
        let mut stranger = Client::control(&controller::endpoint(database)?, ClientRole::Desktop).await?;
        if stranger.call::<ScreenChunk>(Method::DesktopScreenRead, &read, "ScreenChunk").await.is_ok() {
            return Err("foreign connection read a frozen screenshot".into());
        }
        let first: ScreenChunk = client.call(Method::DesktopScreenRead, &read, "ScreenChunk").await?;
        if !first.data_base64.starts_with("iVBORw0KGgo") { return Err("expected a PNG".into()); }
        let pattern = std::env::var_os("DZC_TEST_XVFB_PATTERN").is_some();
        if pattern && first.finished { return Err("pattern fixture did not span multiple IPC frames".into()); }
        let mut chunk = first;
        while !chunk.finished {
            let offset = chunk.next_offset;
            read["offset"] = offset.into();
            chunk = client.call(Method::DesktopScreenRead, &read, "ScreenChunk").await?;
            if chunk.next_offset <= offset { return Err("screenshot chunk did not advance".into()); }
        }
        if chunk.next_offset != start.png_bytes { return Err("incomplete frozen image".into()); }
        let mut crop = with_token(&start.token);
        crop["x"] = (start.x - 1).into();
        crop["y"] = start.y.into();
        crop["width"] = 16.into();
        crop["height"] = 8.into();
        if client.call::<ScreenImage>(Method::DesktopScreenCrop, &crop, "ScreenImage").await.is_ok() {
            return Err("out-of-bounds crop accepted".into());
        }
        crop["x"] = start.x.into();
        let selected: ScreenImage = client.call(Method::DesktopScreenCrop, crop, "ScreenImage").await?;
        if (selected.width, selected.height) != (16, 8) || selected.token == start.token {
            return Err("unexpected crop geometry or reused screenshot token".into());
        }
        if client.call::<ScreenChunk>(Method::DesktopScreenRead, &read, "ScreenChunk").await.is_ok() {
            return Err("old token still read cropped pixels".into());
        }
        let finish = with_token(&selected.token);
        let done: ScreenFinishResult = client.call(Method::DesktopScreenFinish, &finish, "ScreenFinishResult").await?;
        if !done.copied || done.recorded != capture { return Err("unexpected screenshot recording outcome".into()); }
        if pattern {
            let check = std::process::Command::new("python3").args(["-c",
                "import io, subprocess; from PIL import Image; data = subprocess.check_output(['xclip','-selection','clipboard','-target','image/png','-o'],timeout=4); im = Image.open(io.BytesIO(data)).convert('RGB'); assert im.size == (16,8) and im.getextrema() == ((234,234),(25,25),(75,75))"
            ]).status()?;
            if !check.success() { return Err("system clipboard pixels differ from the selected rectangle".into()); }
        }
        if capture {
            let page: BodySnapshot = client.call(Method::DesktopHistoryPage, serde_json::json!({"kind":"image","offset":0,"limit":20}), "BodySnapshot").await?;
            let entries: DesktopEntries = client.read_snapshot(page, "DesktopEntries").await?;
            if entries.entries.iter().filter(|item| item.source_app == "DezirClip Screenshot").count() != 1 {
                return Err("expected one recorded screenshot".into());
            }
        }
        let replay = client.call::<ScreenFinishResult>(Method::DesktopScreenFinish, &finish, "ScreenFinishResult").await;
        if replay.is_ok() { return Err("duplicate finish accepted".into()); }
        let other: ScreenImage = stranger.call(Method::DesktopScreenBegin, &base, "ScreenImage").await?;
        let _: OperationResult = stranger.call(Method::DesktopScreenCancel, with_token(&other.token), "OperationResult").await?;
        let _: ScreenImage = stranger.call(Method::DesktopScreenBegin, &base, "ScreenImage").await?;
        drop(stranger);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let recovered = loop {
            match client.call::<ScreenImage>(Method::DesktopScreenBegin, &base, "ScreenImage").await {
                Ok(image) => break image,
                Err(err) if tokio::time::Instant::now() < deadline
                    && serde_json::from_str::<WireErrorCode>(&err.to_string()).ok() == Some(WireErrorCode::Busy) =>
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await,
                Err(err) => return Err(err.into()),
            }
        };
        let private: PrivacyState = client.call(Method::PrivacySet, serde_json::json!({"active": true}), "PrivacyState").await?;
        let mut revoked = with_token(&recovered.token);
        revoked["expected_epoch"] = private.privacy_epoch.into();
        revoked["offset"] = 0.into();
        if client.call::<ScreenChunk>(Method::DesktopScreenRead, revoked, "ScreenChunk").await.is_ok() {
            return Err("revoked frame survived privacy transition".into());
        }
        let _: PrivacyState = client.call(Method::PrivacySet, serde_json::json!({"active": false}), "PrivacyState").await?;
        Ok(())
    }

    async fn exercise_library(client: &mut Client, database: &Path, state: &RuntimeState) -> Result<(), Box<dyn Error>> {
        let base = serde_json::json!({ "service_instance_id": state.service_instance_id, "expected_epoch": state.privacy_epoch });
        let entry = |id: i64, text: &str, tag: &str| serde_json::json!({
            "id":id, "content_type":"text", "content":text, "tags":[tag], "created_at":1, "updated_at":1
        });
        let fixture = database.with_file_name("library.json");
        std::fs::write(&fixture, serde_json::to_vec(&serde_json::json!({"version":"dezirclip-export-v2","exported_at":1,
            "entries":[entry(21,"matched keep","catalog"),entry(22,"matched remove","catalog"),entry(23,"unrelated","external")]}))?)?;
        let mut import = base.clone();
        import["path"] = serde_json::to_value(&fixture)?;
        import["mode"] = "replace".into();
        let _: BackupImportResult = client.call(Method::DesktopBackupImport, import, "BackupImportResult").await?;
        let mut library = base.clone();
        library["name"] = "catalog".into();
        let mut color = library.clone();
        color["color"] = "#26abcd".into();
        let _: OperationResult = client.call(Method::DesktopTagsColor, color, "OperationResult").await?;
        let colors: BodySnapshot = client.call(Method::DesktopTagsColors, &base, "BodySnapshot").await?;
        let colors: LibraryColors = client.read_snapshot(colors, "LibraryColors").await?;
        let counts: BodySnapshot = client.call(Method::DesktopTagsCounts, &base, "BodySnapshot").await?;
        let counts: LibraryCounts = client.read_snapshot(counts, "LibraryCounts").await?;
        if colors.values["catalog"] != "#26abcd" || counts.values["catalog"] != 2 {
            return Err("tag metadata mismatch".into());
        }
        if client.call::<LibraryDeleted>(Method::DesktopTagsDeleteContents, &library, "LibraryDeleted").await.is_ok() {
            return Err("system-tag deletion accepted an ordinary tag".into());
        }
        let descriptor: BodySnapshot = client.call(Method::DesktopTagsPage, serde_json::json!({
            "service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,"tag":"catalog",
            "query":"","sort_by":"time","limit":10,"offset":0
        }), "BodySnapshot").await?;
        let page: DesktopTagItemsPage = client.read_snapshot(descriptor, "DesktopTagItemsPage").await?;
        let keep = page.items.first().ok_or("missing tagged entry")?.id;
        library["scope"] = "all".into();
        library["ids"] = serde_json::json!([keep]);
        library["query"] = "matched".into();
        let removed: LibraryDeleted = client.call(Method::DesktopTagsDeleteEntries, library, "LibraryDeleted").await?;
        let count: LibraryCount = client.call(Method::DesktopHistoryCount, base, "LibraryCount").await?;
        if removed.deleted_count != 1 || removed.cleanup_failed || count.count != 2 {
            return Err("filtered deletion did not preserve the exclusion and unrelated row".into());
        }
        Ok(())
    }

    async fn exercise_settings(client: &mut Client, state: &RuntimeState) -> Result<(), Box<dyn Error>> {
        let params = serde_json::json!({"service_instance_id": state.service_instance_id, "expected_epoch": state.privacy_epoch});
        let before: CaptureSettingValues = client.call(Method::DesktopSettingsCaptureGet, &params, "CaptureSettingValues").await?;
        let mut update = params.clone();
        update["key"] = "app.capture_files".into();
        update["value"] = (!before.capture_files).to_string().into();
        let _: OperationResult = client.call(Method::DesktopSettingsCaptureSet, &update, "OperationResult").await?;
        let after: CaptureSettingValues = client.call(Method::DesktopSettingsCaptureGet, &params, "CaptureSettingValues").await?;
        if after.capture_files == before.capture_files { return Err("capture policy was not applied".into()); }
        update["expected_epoch"] = (state.privacy_epoch + 1).into();
        update["value"] = before.capture_files.to_string().into();
        let rejected = client.call::<OperationResult>(Method::DesktopSettingsCaptureSet, &update, "OperationResult").await;
        if rejected.err().and_then(|err| serde_json::from_str::<WireErrorCode>(&err.to_string()).ok()) != Some(WireErrorCode::PrivacyEpochChanged) {
            return Err("stale capture settings write was not rejected".into());
        }
        Ok(())
    }

    async fn exercise_manual_ocr(client: &mut Client, database: &Path, state: &RuntimeState) -> Result<(), Box<dyn Error>> {
        let source = database.with_file_name("image.json");
        std::fs::write(&source, include_bytes!("../tests/fixtures/manual-ocr.json"))?;
        let _: BackupImportResult = client.call(Method::DesktopBackupImport, serde_json::json!({
            "service_instance_id":state.service_instance_id, "expected_epoch":state.privacy_epoch, "path":source, "mode":"replace"
        }), "BackupImportResult").await?;
        let page: BodySnapshot = client.call(Method::DesktopHistoryPage,
            serde_json::json!({"limit": 1, "offset": 0, "kind": "image"}), "BodySnapshot").await?;
        let page: DesktopEntries = client.read_snapshot(page, "DesktopEntries").await?;
        let id = page.entries.first().ok_or("imported image missing")?.id;
        let mut params = serde_json::json!({"service_instance_id":state.service_instance_id, "expected_epoch":state.privacy_epoch + 1,
            "id":id, "reveal":false});
        let rejected = client.call::<BodySnapshot>(Method::DesktopOcrRecognize, &params, "BodySnapshot").await;
        if rejected.err().and_then(|err| serde_json::from_str::<WireErrorCode>(&err.to_string()).ok()) != Some(WireErrorCode::PrivacyEpochChanged) {
            return Err("stale OCR request was not rejected".into());
        }
        params["expected_epoch"] = state.privacy_epoch.into();
        let engine: OcrEngineInfo = client.call(Method::DesktopOcrEngine, serde_json::json!({}), "OcrEngineInfo").await?;
        if !engine.available {
            if std::env::var_os("DZC_TEST_REQUIRE_OCR").is_some() { return Err("native OCR engine required".into()); }
            return Ok(());
        }
        for method in [Method::DesktopOcrRecognize, Method::DesktopOcrStatus] {
            let body: BodySnapshot = client.call(method, &params, "BodySnapshot").await?;
            let output: ManualOcrText = client.read_snapshot(body, "ManualOcrText").await?;
            if output.status != "done" || !output.text.unwrap_or_default().to_uppercase().contains("DESKTOP") {
                return Err("native OCR text did not match the fixture".into());
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn exercise_queue(client: &mut Client, state: &RuntimeState, executable: &Path, capture: bool) -> Result<(), Box<dyn Error>> {
        use std::{io::{BufRead, BufReader}, process::{Child, Command, Stdio}};
        struct Window(Child, u32);
        impl Window {
            fn start(exe: &Path, method: &str, text: &str) -> Result<Self, Box<dyn Error>> {
                let mut child = Command::new(exe).args([method, text]).stdout(Stdio::piped()).spawn()?;
                let mut line = String::new();
                BufReader::new(child.stdout.as_mut().ok_or("fixture stdout")?).read_line(&mut line)?;
                Ok(Self(child, line.trim().strip_prefix("READY ").ok_or("fixture not ready")?.parse()?))
            }
        }
        impl Drop for Window {
            fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
        }
        let base = serde_json::json!({"service_instance_id":state.service_instance_id, "expected_epoch":state.privacy_epoch});
        let desc: BodySnapshot = client.call(Method::DesktopHistoryPage, serde_json::json!({"query":"unrelated","limit":1,"offset":0}), "BodySnapshot").await?;
        let page: DesktopEntries = client.read_snapshot(desc, "DesktopEntries").await?;
        let id = page.entries.first().ok_or("queue source missing")?.id;
        let mut set = base.clone();
        set["ids"] = serde_json::json!([id, id]);
        let _: OperationResult = client.call(Method::DesktopQueueSet, set, "OperationResult").await?;
        for (index, method) in ["ctrl_v", "shift_insert"].into_iter().enumerate() {
            for (key, value) in [("app.paste_method",method),("app.delete_after_paste",if index == 1 {"true"} else {"false"})] {
                let mut config = base.clone();
                config["key"] = key.into();
                config["value"] = value.into();
                let _: OperationResult = client.call(Method::DesktopQueueConfigure, config, "OperationResult").await?;
            }
            let mut window = Window::start(executable, method, "unrelated")?;
            let dest: QueueTarget = client.call(Method::DesktopPasteTarget, &base, "QueueTarget").await?;
            if dest.x11 != Some(window.1) {return Err("target identity mismatch".into());}
            let mut q: QueueState = client.call(Method::DesktopQueueGet, &base, "QueueState").await?;
            let mut next = base.clone();
            next["target"] = serde_json::to_value(dest)?;
            next["expected_revision"] = q.revision.into();
            if index == 0 {
                let decoy = Window::start(executable, method, "unrelated")?;
                if client.call::<QueueStepResult>(Method::DesktopQueueNext, &next, "QueueStepResult").await.is_ok() {return Err("changed focus accepted".into());}
                drop(decoy);
                if !Command::new(executable).args(["--focus",&window.1.to_string()]).status()?.success() {return Err("fixture focus failed".into());}
                q = client.call(Method::DesktopQueueGet, &base, "QueueState").await?;
                if q.ids != [id,id] {return Err("failed paste consumed queue".into());}
                next["expected_revision"] = q.revision.into();
            }
            let step: QueueStepResult = client.call(Method::DesktopQueueNext, &next, "QueueStepResult").await?;
            if step.status != "pasted" || step.id != Some(id) || step.remaining as usize != 1-index || !window.0.wait()?.success() {
                return Err("keystroke or queue delivery mismatch".into());
            }
            if client.call::<QueueStepResult>(Method::DesktopQueueNext, &next, "QueueStepResult").await.is_ok() {return Err("replayed queue step".into());}
        }
        let count: LibraryCount = client.call(Method::DesktopHistoryCount, &base, "LibraryCount").await?;
        if count.count != 1 {return Err("delete-after-paste missed or deleted the wrong row".into());}
        if capture {
            use std::io::Write;
            let mut config = base.clone();
            config["key"] = "app.sequential_mode".into();
            config["value"] = "true".into();
            let _: OperationResult = client.call(Method::DesktopQueueConfigure, &config, "OperationResult").await?;
            let mut owner = Command::new("xclip").args(["-selection","clipboard","-i"]).stdin(Stdio::piped())
                .stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
            owner.stdin.take().ok_or("fixture clipboard stdin")?.write_all(b"Queue RAM fixture")?;
            if !owner.wait()?.success() {return Err("clipboard source failed".into());}
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            let q = loop {
                let q: QueueState = client.call(Method::DesktopQueueGet, &base, "QueueState").await?;
                if !q.ids.is_empty() {break q;}
                if tokio::time::Instant::now() >= deadline {return Err("captured text did not enter the service queue".into());}
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            };
            if q.ids.len() != 1 || q.ids[0] >= 0 {return Err("expected one RAM-only queue entry".into());}
            let mut target = Window::start(executable, "shift_insert", "Queue RAM fixture")?;
            let dest: QueueTarget = client.call(Method::DesktopPasteTarget, &base, "QueueTarget").await?;
            let mut next = base.clone();
            next["expected_revision"] = q.revision.into();
            next["target"] = serde_json::to_value(dest)?;
            let result: QueueStepResult = client.call(Method::DesktopQueueNext, next, "QueueStepResult").await?;
            if result.status != "pasted" || result.id != Some(q.ids[0]) || !target.0.wait()?.success() {
                return Err("RAM-only queue delivery failed".into());
            }
            config["value"] = "false".into();
            let _: OperationResult = client.call(Method::DesktopQueueConfigure, config, "OperationResult").await?;
        }
        Ok(())
    }

    async fn exercise_backups(client: &mut Client, database: &Path, state: &RuntimeState) -> Result<(), Box<dyn Error>> {
        let source = database.with_file_name("source.json");
        let encrypted = database.with_file_name("round-trip.dzc");
        let fixture = serde_json::to_vec(&serde_json::json!({"version":"dezirclip-export-v2", "exported_at":1,
            "entries":[{"id":1, "content_type":"text", "content":"fixture 汉字", "created_at":1, "updated_at":1}]}))?;
        std::fs::write(&source, &fixture)?;
        let base = |path: &Path| serde_json::json!({"service_instance_id":state.service_instance_id,
            "expected_epoch":state.privacy_epoch, "path":path});
        let preview: BackupPreview = client.call(Method::DesktopBackupPreview, base(&source), "BackupPreview").await?;
        if preview.entries.first().map(|row|row.content.as_str()) != Some("fixture 汉字") || std::fs::read(&source)? != fixture {
            return Err("backup preview modified the source or returned the wrong content".into());
        }
        let mut import = base(&source);
        import["mode"] = "merge".into();
        let result: BackupImportResult = client.call(Method::DesktopBackupImport, import, "BackupImportResult").await?;
        if result.imported != 1 {return Err("backup import count mismatch".into());}
        let mut export = base(&encrypted);
        export["format"] = "encrypted".into();
        export["passphrase"] = "synthetic long password".into();
        let written: BackupExportResult = client.call(Method::DesktopBackupExport, export, "BackupExportResult").await?;
        if written.count != 1 || !written.encrypted {return Err("encrypted backup not written".into());}
        let mut restore = base(&encrypted);
        restore["mode"] = "replace".into();
        restore["passphrase"] = "wrong".into();
        if client.call::<BackupImportResult>(Method::DesktopBackupImport, &restore, "BackupImportResult").await.is_ok() {
            return Err("wrong backup password accepted".into());
        }
        restore["passphrase"] = "synthetic long password".into();
        let restored: BackupImportResult = client.call(Method::DesktopBackupImport, restore, "BackupImportResult").await?;
        if restored.imported != 1 || restored.mode != "replace" {return Err("encrypted restore mismatch".into());}
        Ok(())
    }
}
