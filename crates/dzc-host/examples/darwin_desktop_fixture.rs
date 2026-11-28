//! Runs the real Host with an isolated profile and a main-thread text clipboard.

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> { darwin::run() }

#[cfg(not(target_os = "macos"))]
fn main() -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "requires macOS"))
}

#[cfg(target_os = "macos")]
#[path = "support/darwin_profile.rs"]
mod profile_fixture;

#[cfg(target_os = "macos")]
mod darwin {
    use dzc_core::{api::{ClipboardEntry, PrivacySessionCommand}, ports::ClipboardPayload};
    use dzc_host::{Host, HostOptions, darwin_desktop::{serve_darwin_desktop, wire_error}, wire::*};
    use dzc_host::darwin_clipboard::DarwinClipboardPump;
    use dzc_host::darwin_profile::DarwinProfile;
    use dzc_platform::darwin_keychain::DarwinKeyProvider;
    use security_framework::os::macos::keychain::CreateOptions;
    use serde_json::{Value, json};
    use std::{io::{self, Read, Write}, path::PathBuf, time::Duration};
    use tokio::sync::mpsc;

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
        let first = std::env::args_os().nth(1).ok_or("private fixture directory required")?;
        match first.to_str() {
            Some("--verify-profile") => return super::profile_fixture::verify(),
            Some("--profile-child") => return super::profile_fixture::child(),
            _ => {}
        }
        let directory = PathBuf::from(first);
        let (clipboard, actions) = DarwinClipboardPump::open()?;
        if actions.copy(ClipboardPayload::Text(String::new())) != Err(WireErrorCode::Unavailable) {
            return Err("main-thread copy dispatch did not refuse a blocking wait".into());
        }
        let password = uuid::Uuid::new_v4().simple().to_string();
        let keychain = CreateOptions::new().password(&password).prompt_user(false)
            .create(directory.join("fixture.keychain-db"))?;
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let _entered = runtime.enter();
        let profile = DarwinProfile::open(&directory.join("profile"), |id| DarwinKeyProvider::for_profile(keychain, id))?;
        let history = profile.history.clone();
        let marker = format!("Swift-to-Host 示例 {}", uuid::Uuid::new_v4());
        let text = format!("{marker}\n{}", "x".repeat(200_000));
        let entry: ClipboardEntry = serde_json::from_value(json!({
            "id": 0, "content_type": "text", "content": text, "preview": marker,
            "source_app": "Darwin Host fixture", "timestamp": 1, "is_pinned": false, "tags": []
        }))?;
        let id = history.save(&entry, None)?;
        let host = Host::new(HostOptions {
            mode: HostMode::Service, history: history.clone(), directory: profile.directory.clone(), actions,
        }).map_err(wire_error)?;
        host.publish(EventKind::EntryAdded, EventData { entry_id: Some(id), status: None }).map_err(wire_error)?;
        let grant = host.grants().issue(ClientRole::Desktop, vec![
            Permission::HistoryRead, Permission::DesktopHistory, Permission::ClipboardCopy,
        ], Duration::from_secs(90)).map_err(wire_error)?;
        let mut commands = parent_commands()?;
        // Keep the fixture commands and extra receipt fields outside the shared service loop.
        send_receipt(&json!({"instance": host.instance_id(), "grant": grant.token(), "entry_id": id, "marker": marker, "socket": profile.endpoint.address()}))?;
        let owner = host.clone();
        let fixture_grant = grant.clone();
        runtime.block_on(serve_darwin_desktop(host, profile.listener, clipboard, None, grant, async move {
            while let Some(command) = commands.recv().await {
                match command {
                    b'p' => {
                        history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, unix_milliseconds()?)
                            .map_err(io::Error::other)?;
                        let state = owner.snapshot().map_err(wire_error)?;
                        send_receipt(&json!({"privacy_epoch": state.privacy_epoch}))?;
                    }
                    b'r' => {
                        owner.grants().revoke(&fixture_grant);
                        send_receipt(&json!({"revoked": true}))?;
                    }
                    _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
                }
            }
            Ok(())
        }))?;
        Ok(())
    }

    fn unix_milliseconds() -> io::Result<i64> {
        let elapsed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(io::Error::other)?;
        i64::try_from(elapsed.as_millis()).map_err(io::Error::other)
    }
}
