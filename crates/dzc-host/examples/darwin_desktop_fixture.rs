//! Runs the real Host with an isolated profile and a main-thread text clipboard.

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> { darwin::run() }

#[cfg(not(target_os = "macos"))]
fn main() -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "requires macOS"))
}

#[cfg(target_os = "macos")]
mod darwin {
    use dzc_core::{api::{ClipboardEntry, HistoryService, PrivacySessionCommand}, ports::ClipboardPayload};
    use dzc_host::{Host, HostActions, HostOptions, serve_listener, wire::*};
    use dzc_platform::{darwin_clipboard::MainThreadClipboard, ipc::Endpoint};
    use serde_json::{Value, json};
    use std::{io::{self, Read, Write}, path::PathBuf, sync::{Arc, mpsc as reply}, time::Duration};
    use tokio::sync::{mpsc, watch};

    struct WriteText {
        text: String,
        completed: reply::SyncSender<Result<(), WireErrorCode>>,
    }

    struct Clipboard(mpsc::Sender<WriteText>);
    impl HostActions for Clipboard {
        fn platform_capabilities(&self) -> Value { json!({"native_text_clipboard": true}) }
        fn can_copy(&self) -> bool { true }
        fn copy(&self, payload: ClipboardPayload) -> Result<(), WireErrorCode> {
            let ClipboardPayload::Text(text) = payload else { return Err(WireErrorCode::UnsupportedCapability); };
            let (completed, result) = reply::sync_channel(1);
            self.0.try_send(WriteText { text, completed }).map_err(|_| WireErrorCode::Unavailable)?;
            result.recv().map_err(|_| WireErrorCode::Unavailable)?
        }
        fn can_paste(&self) -> bool { false }
        fn paste(&self) -> Result<(), WireErrorCode> { Err(WireErrorCode::UnsupportedCapability) }
        fn stop_helpers(&self) -> Result<(), WireErrorCode> { Ok(()) }
        fn revoke_private_exports(&self) -> Result<(), WireErrorCode> { Ok(()) }
    }

    fn wire_error(code: WireErrorCode) -> io::Error { io::Error::other(format!("{code:?}")) }

    fn send_receipt(value: &Value) -> io::Result<()> {
        let mut output = io::stdout().lock();
        writeln!(output, "{value}")?;
        output.flush()
    }

    fn parent_commands() -> io::Result<mpsc::Receiver<u8>> {
        let (sender, commands) = mpsc::channel(1);
        std::thread::Builder::new().name("darwin-fixture-parent".into()).spawn(move || {
            let mut input = io::stdin().lock();
            let mut command = [0];
            while input.read_exact(&mut command).is_ok() {
                if sender.blocking_send(command[0]).is_err() { break; }
            }
        })?;
        Ok(commands)
    }

    pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
        let directory = PathBuf::from(std::env::args_os().nth(1).ok_or("private fixture directory required")?);
        let endpoint = Endpoint::in_directory(&directory)?;
        let mut clipboard = MainThreadClipboard::open()?;
        let history = HistoryService::open(&directory.join("fixture.db"))?;
        let marker = format!("Swift-to-Host 示例 {}", uuid::Uuid::new_v4());
        let text = format!("{marker}\n{}", "x".repeat(200_000));
        let entry: ClipboardEntry = serde_json::from_value(json!({
            "id": 0, "content_type": "text", "content": text, "preview": marker,
            "source_app": "Darwin Host fixture", "timestamp": 1, "is_pinned": false, "tags": []
        }))?;
        let id = history.save(&entry, None)?;
        let (writer, mut writes) = mpsc::channel(1);
        let host = Host::new(HostOptions {
            mode: HostMode::Service, history: history.clone(), directory, actions: Arc::new(Clipboard(writer)),
        }).map_err(wire_error)?;
        host.publish(EventKind::EntryAdded, EventData { entry_id: Some(id), status: None }).map_err(wire_error)?;
        let grant = host.grants().issue(ClientRole::Desktop, vec![
            Permission::HistoryRead, Permission::DesktopHistory, Permission::ClipboardCopy,
        ], Duration::from_secs(90)).map_err(wire_error)?;
        let mut commands = parent_commands()?;
        tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
            let listener = endpoint.bind_for_parent()?;
            // Discovery is the socket path alone. The grant stays on the
            // private pipe inherited from the launcher.
            send_receipt(&json!({"instance": host.instance_id(), "grant": grant.token(), "entry_id": id, "marker": marker}))?;
            let (shutdown, receiver) = watch::channel(false);
            let server = serve_listener(host.clone(), listener, receiver);
            tokio::pin!(server);
            let mut drain: Option<tokio::task::JoinHandle<Result<(), WireErrorCode>>> = None;
            loop {
                tokio::select! {
                    result = &mut server => {
                        result?;
                        if let Some(worker) = drain {
                            worker.await.map_err(io::Error::other)?.map_err(wire_error)?;
                        }
                        return Ok(());
                    },
                    Some(write) = writes.recv() => {
                        let result = clipboard.write_text(&write.text).map_err(|_| WireErrorCode::Unavailable);
                        let _ = write.completed.send(result);
                    },
                    command = commands.recv(), if drain.is_none() => match command {
                        Some(b'p') => {
                            let now = unix_milliseconds()?;
                            history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, now)
                                .map_err(io::Error::other)?;
                            let state = host.snapshot().map_err(wire_error)?;
                            send_receipt(&json!({"privacy_epoch": state.privacy_epoch}))?;
                        },
                        Some(b'r') => {
                            host.grants().revoke(&grant);
                            send_receipt(&json!({"revoked": true}))?;
                        },
                        None => {
                            host.grants().revoke(&grant);
                            let _ = shutdown.send(true);
                            let shutting_down = host.clone();
                            drain = Some(tokio::task::spawn_blocking(move || shutting_down.shutdown()));
                        },
                        Some(_) => return Err(io::Error::from(io::ErrorKind::InvalidData)),
                    },
                }
            }
        })?;
        Ok(())
    }

    fn unix_milliseconds() -> io::Result<i64> {
        let elapsed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(io::Error::other)?;
        i64::try_from(elapsed.as_millis()).map_err(io::Error::other)
    }
}
