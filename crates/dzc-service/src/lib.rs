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

struct NativeActions { writer: dzc_platform::runtime::EmbeddedClipboardWriter }
impl HostActions for NativeActions {
    fn platform_capabilities(&self) -> serde_json::Value { serde_json::to_value(dzc_platform::runtime::standalone_capabilities()).unwrap_or_default() }
    fn can_copy(&self) -> bool { self.writer.capability().is_available() }
    fn copy(&self, payload: ClipboardPayload) -> Result<(), WireErrorCode> { self.writer.write_suppressing_recapture(payload).map_err(|_| WireErrorCode::Unavailable) }
    fn can_paste(&self) -> bool { dzc_platform::runtime::NativePasteController.capability().is_available() }
    fn paste(&self) -> Result<(), WireErrorCode> { dzc_platform::runtime::NativePasteController.paste().map_err(|_| WireErrorCode::Unavailable) }
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
        if !options.no_capture {
            let (manager, ready) = CaptureManager::start(&host).map_err(|_| "HostCaptureOwnershipUnavailable")?;
            if ready.is_err() {
                if manager.stop().is_err() { return Err("HostCaptureStopUnconfirmed".to_string()); }
                return Err("HostCaptureUnavailable".to_string());
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
