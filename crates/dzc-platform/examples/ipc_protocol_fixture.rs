//! Cross-language framing fixture with native desktop checks on Darwin.
use dzc_platform::ipc::{read_frame, write_frame, Endpoint};
use dzc_protocol::wire::{self, HelloResult, Method, Permission, ProtocolVersion, Request, Response, RuntimeState};
use std::{io::{self, Write}, path::Path, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if matches!(std::env::args().nth(1).as_deref(), Some("--capture-lease-fixture" | "--capture-lease-check")) {
        let _lease = match dzc_platform::ipc::CaptureLease::acquire() {
            Ok(lease) => lease,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => { eprintln!("capture-lease-busy"); std::process::exit(3); },
            Err(error) => return Err(error.into()),
        };
        println!("capture-lease-acquired"); io::stdout().flush()?;
        if std::env::args().nth(1).as_deref() == Some("--capture-lease-fixture") { let _ = io::stdin().read_line(&mut String::new())?; }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--identity-client") {
        return identity_client();
    }
    let directory = std::env::args_os().nth(1).ok_or("private fixture directory required")?;
    #[cfg(target_os = "macos")]
    let expected_parent = if std::env::args().nth(2).as_deref() == Some("--darwin-parent") {
        let pid = unsafe { libc::getppid() };
        if pid <= 1 { return Err("missing fixture parent".into()); }
        Some(pid as u32)
    } else { None };
    #[cfg(target_os = "macos")]
    let _capture_lease = expected_parent.map(|_| dzc_platform::ipc::CaptureLease::acquire()).transpose()?;
    #[cfg(target_os = "macos")]
    let parent_closed = expected_parent.map(|_| watch_parent_lifetime()).transpose()?;
    let requests = async {
        let endpoint = Endpoint::in_directory(Path::new(&directory))?;
        let mut listener = endpoint.bind()?;
        println!("{}", endpoint.address().display());
        io::stdout().flush()?;
        let mut stream = tokio::time::timeout(Duration::from_secs(20), async {
            #[cfg(target_os = "macos")]
            if let Some(pid) = expected_parent {
                loop {
                    let stream = listener.accept().await?;
                    if stream.authenticate_after_read()?.pid() == Some(pid) {
                        return Ok(stream);
                    }
                }
            }
            listener.accept().await
        }).await??;
        for index in 0..3 {
            let bytes = read_frame(&mut stream, Duration::from_secs(5)).await?;
            let _peer = stream.authenticate_after_read()?;
            let request: Request = wire::decode("Request", &bytes).map_err(|_| "invalid fixture request")?;
            let result = match (index, request.method) {
                (0, Method::Hello) => serde_json::to_value(HelloResult {
                    session_id: "synthetic-session".into(), protocol_version: ProtocolVersion { major: 1, minor: 0 },
                    core_version: "fixture".into(), permissions: vec![Permission::HistoryRead],
                    state: RuntimeState { service_instance_id: "synthetic-instance".into(), history_revision: 3, privacy_epoch: 2, event_sequence: 0 },
                    max_frame_bytes: wire::MAX_FRAME_BYTES as u32,
                })?,
                #[cfg(target_os = "macos")]
                (1, Method::Health) if expected_parent.is_some() => {
                    let marker = format!("darwin-pasteboard-{}", uuid::Uuid::new_v4());
                    let mut clipboard = dzc_platform::darwin_clipboard::MainThreadClipboard::open()?;
                    clipboard.write_text(&marker)?;
                    if clipboard.read_text()? != marker { return Err("native clipboard readback differs".into()); }
                    serde_json::json!({"clipboard_text": marker})
                }
                (1, Method::Health) => serde_json::json!({}),
                #[cfg(target_os = "macos")]
                (2, Method::SurfaceHistoryList) if expected_parent.is_some() => {
                    verify_darwin_keychain()?;
                    serde_json::json!({"entries": [], "keychain_verified": true})
                }
                (2, Method::SurfaceHistoryList) => serde_json::json!({"entries":[]}),
                _ => return Err("unexpected fixture method".into()),
            };
            let response = Response::success(request.request_id, result);
            let bytes = wire::encode("Response", &response).map_err(|_| "invalid fixture response")?;
            write_frame(&mut stream, &bytes, Duration::from_secs(5)).await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        #[cfg(target_os = "macos")]
        if let Some(parent_closed) = parent_closed {
            return tokio::select! {
                _ = parent_closed => Ok(()),
                result = requests => result,
            };
        }
        requests.await
    })
}

#[cfg(target_os = "macos")]
fn watch_parent_lifetime() -> io::Result<tokio::sync::oneshot::Receiver<()>> {
    use std::io::Read;
    let (closed, observed) = tokio::sync::oneshot::channel();
    std::thread::Builder::new().name("darwin-parent-lifetime".into()).spawn(move || {
        // The read end belongs to this child. The launcher retains the only
        // write end and closes it on shutdown, including unexpected exit.
        let _ = io::stdin().read(&mut [0u8]);
        let _ = closed.send(());
    })?;
    Ok(observed)
}

#[cfg(target_os = "macos")]
fn verify_darwin_keychain() -> Result<(), Box<dyn std::error::Error>> {
    use dzc_core::api::{ClipboardEntry, HistoryService, PrivacyAccessIntent, PrivacyError, SensitiveAccess};
    use dzc_core::crypto::{EncryptionError, KeyProvider, KEYCHAIN_ENCRYPT_PREFIX};
    use dzc_platform::darwin_keychain::DarwinKeyProvider;
    use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("probe.keychain-db");
    let password = uuid::Uuid::new_v4().simple().to_string();
    let profile_id = uuid::Uuid::new_v4();
    let marker = format!("keychain{}", uuid::Uuid::new_v4().simple());
    let keychain = CreateOptions::new().password(&password).prompt_user(false).create(&path)?;
    let provider = DarwinKeyProvider::for_profile(keychain, profile_id)?;
    if provider.seal_text(&marker) != Err(EncryptionError::KeyUnavailable) {
        return Err("a missing key was silently initialized while sealing".into());
    }
    provider.provision_new_profile()?;
    let first_cipher = provider.seal_text(&marker)?;
    provider.provision_new_profile()?;
    if !first_cipher.starts_with(KEYCHAIN_ENCRYPT_PREFIX)
        || provider.open_text(&first_cipher).as_deref() != Some(marker.as_str()) {
        return Err("Darwin keychain replaced an existing key during provisioning".into());
    }
    drop(provider);

    let mut keychain = SecKeychain::open(&path)?;
    keychain.unlock(Some(&password))?;
    let provider = DarwinKeyProvider::for_profile(keychain, profile_id)?;
    if provider.open_text(&first_cipher).as_deref() != Some(marker.as_str()) {
        return Err("reopening Darwin keychain lost the original credential".into());
    }
    let provider = Box::leak(Box::new(provider));
    dzc_core::crypto::install_key_provider(provider)?;
    let database = directory.path().join("history.sqlite3");
    let history = HistoryService::open(&database)?;
    let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
        "id": 0, "content_type": "text", "content": marker, "preview": marker,
        "source_app": "signed fixture", "timestamp": 1, "is_pinned": false, "tags": ["sensitive"],
    }))?;
    let id = history.save(&entry, None)?;
    let reader = rusqlite::Connection::open_with_flags(&database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let stored: String = reader.query_row("SELECT content FROM clipboard_history WHERE id=?1", [id], |row| row.get(0))?;
    if !stored.starts_with(KEYCHAIN_ENCRYPT_PREFIX) || stored.contains(&marker) {
        return Err("Core persisted the private record outside the Keychain envelope".into());
    }
    let denied = history.privacy().authorize_read(
        PrivacyAccessIntent::InteractiveRead, SensitiveAccess::Denied, None, 2)?;
    if history.get_entry_authorized(id, &denied, 2)?.is_some() {
        return Err("Core revealed the protected record without explicit authorization".into());
    }
    let revealed = history.privacy().authorize_read(
        PrivacyAccessIntent::InteractiveRead, SensitiveAccess::ExplicitUserAction, None, 2)?;
    if history.get_entry_authorized(id, &revealed, 2)?.as_ref().map(|item| item.content.as_str()) != Some(marker.as_str()) {
        return Err("authorized Core read did not recover the protected record".into());
    }
    let locked = std::process::Command::new("/usr/bin/security").arg("lock-keychain").arg(&path).status()?;
    if !locked.success() || provider.open_text(&stored).is_some()
        || provider.seal_text(&marker) != Err(EncryptionError::KeyUnavailable)
        || provider.provision_new_profile().is_ok() {
        return Err("a locked Darwin keychain did not fail closed".into());
    }
    if !matches!(history.get_entry_authorized(id, &revealed, 2), Err(PrivacyError::ProtectedRecordUnavailable)) {
        return Err("Core did not report a protected record as unavailable after keychain lock".into());
    }
    let count = history.get_count()?;
    let pending = ClipboardEntry { content: "different fixture payload".into(), ..entry };
    if history.save(&pending, None).is_ok() || history.get_count()? != count {
        return Err("Core persisted a sensitive mutation without the keychain".into());
    }
    Ok(())
}

fn identity_client() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os().nth(2).ok_or("private fixture directory required")?;
    let expected_pid: u32 = std::env::args().nth(3).ok_or("server PID required")?.parse()?;
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let endpoint = Endpoint::in_directory(Path::new(&directory))?;
        let mut stream = endpoint.connect(Duration::from_secs(2)).await?;
        if stream.authenticate_after_read()?.pid() != Some(expected_pid) { return Err("wrong authenticated server PID".into()); }
        for (index, (method, params)) in [
            (Method::Hello, serde_json::json!({"protocol_version":{"major":1,"minor":0},"client_version":"fixture","client_role":"cli","capabilities":[],"required_capabilities":[]})),
            (Method::Health, serde_json::json!({})),
            (Method::SurfaceHistoryList, serde_json::json!({"limit":20,"offset":0})),
        ].into_iter().enumerate() {
            let request = Request { request_id: index.to_string(), method, params };
            let bytes = wire::encode("Request", &request).map_err(|_| "invalid fixture request")?;
            write_frame(&mut stream, &bytes, Duration::from_secs(2)).await?;
            let bytes = read_frame(&mut stream, Duration::from_secs(2)).await?;
            let _: Response = wire::decode("Response", &bytes).map_err(|_| "invalid fixture response")?;
        }
        println!("native server PID verified: {expected_pid}");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}
