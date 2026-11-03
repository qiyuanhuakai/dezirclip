//! Optional per-user service. Linking this library starts no workers.
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
    let options = Options::parse();
    if options.build_info {
        println!("{}", serde_json::to_string(&dzc_protocol::build_info::BuildInfo::current(dzc_protocol::build_info::Component::Service, dzc_core::api::DATABASE_SCHEMA_VERSION)).map_err(|_| "BuildInfoUnavailable")?);
        return Ok(());
    }
    dzc_platform::ipc::ProcessExit::ensure_supported().map_err(|_| "ServiceProcessObservationUnsupported")?;
    let database = options.database.map(Ok).unwrap_or_else(dzc_host::data::database_path)?;
    let directory = dzc_host::data::canonical_directory(&database)?;
    dzc_platform::install_default_key_provider();
    let history = HistoryService::open(&database).map_err(|_| "HostDatabaseUnavailable")?;
    let host = Host::new(HostOptions { mode: HostMode::Service, history, directory: directory.clone(), actions: Arc::new(NativeActions { writer: Default::default() }) }).map_err(|_| "HostInitializationFailed")?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "HostRuntimeUnavailable")?;
    runtime.block_on(async {
        let endpoint = dzc_platform::ipc::Endpoint::for_current_session(&directory).map_err(|_| "HostEndpointUnavailable")?;
        // Bind before publishing grants or starting capture; an existing owner
        // cannot have its credentials replaced by a losing startup attempt.
        let listener = endpoint.bind().map_err(|_| "HostAlreadyRunning")?;
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
        ctrlc::set_handler(move || {
            if let Some(host) = signal_host.upgrade() {
                if host.shutdown().is_ok() { let _ = shutdown.send(true); }
            }
        }).map_err(|_| "HostSignalUnavailable")?;
        dzc_host::serve_listener(host, listener, receiver).await.map_err(|_| "HostTransportFailed".to_string())
    })
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
            controller::start(&database, &executable, true).await?;
            let checks = exercise_tools(&database).await;
            controller::stop(&database).await?;
            checks
        });
        std::fs::remove_dir_all(scratch)?;
        result
    }

    async fn exercise_tools(database: &Path) -> Result<(), Box<dyn Error>> {
        let mut client = Client::control(&controller::endpoint(database)?, ClientRole::Desktop).await?;
        let state: ServiceStatus = client.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
        if state.mode != HostMode::Service || state.capture_owner { return Err("expected an isolated service".into()); }
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
        exercise_backups(&mut client, database, &state.state).await
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
