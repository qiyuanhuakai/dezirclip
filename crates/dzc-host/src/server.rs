use crate::{Host, Session};
use dzc_platform::ipc::{self, Endpoint, Stream};
use dzc_protocol::wire::{self, Method, Request, Response, WireErrorCode};
use std::{io, sync::Arc, time::Duration};
use tokio::{sync::{watch, Semaphore}, task::JoinSet};

/// The caller owns the runtime; no process or worker is started by constructing
/// a Host. Start this future only when embedded IPC or service mode is enabled.
pub async fn serve(host: Arc<Host>, endpoint: Endpoint, shutdown: watch::Receiver<bool>) -> io::Result<()> {
    serve_listener(host, endpoint.bind()?, shutdown).await
}
pub async fn serve_listener(host: Arc<Host>, mut listener: ipc::Listener, mut shutdown: watch::Receiver<bool>) -> io::Result<()> {
    // NamedPipe reserves an additional listening instance below its kernel cap.
    let connections = Arc::new(Semaphore::new(16));
    let dispatch_slots = Arc::new(Semaphore::new(1));
    let (connections_stop, connection_shutdown) = watch::channel(false);
    let mut workers = JoinSet::new();
    loop {
        if *shutdown.borrow() || host.is_stopping() && host.capture_stopped() { break; }
        let permit = tokio::select! {
            changed = shutdown.changed() => { if changed.is_err() || *shutdown.borrow() { break; } else { continue; } },
            permit = connections.clone().acquire_owned() => permit.map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?,
            Some(_) = workers.join_next(), if !workers.is_empty() => continue,
        };
        let stream = tokio::select! {
            changed = shutdown.changed() => { if changed.is_err() || *shutdown.borrow() { break; } else { continue; } },
            stream = listener.accept() => match stream { Ok(stream) => stream, Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue, Err(error) => return Err(error) },
            Some(_) = workers.join_next(), if !workers.is_empty() => continue,
        };
        let host = host.clone();
        let mut stopped = connection_shutdown.clone();
        let dispatch_slots = dispatch_slots.clone();
        workers.spawn(async move {
            let _permit = permit;
            tokio::select! {
                _ = connection(host, stream, dispatch_slots) => (),
                _ = stopped.changed() => (),
            }
        });
    }
    drop(listener);
    let _ = connections_stop.send(true);
    // Request handlers may still be writing a stop acknowledgement. All framed
    // I/O and serialized operations are bounded; retain the Host until drained.
    while workers.join_next().await.is_some() {}
    Ok(())
}

async fn send(stream: &mut Stream, response: Response) -> io::Result<()> {
    let bytes = wire::encode("Response", &response).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    ipc::write_frame(stream, &bytes, Duration::from_secs(5)).await
}

async fn connection(host: Arc<Host>, mut stream: Stream, dispatch_slots: Arc<Semaphore>) -> io::Result<()> {
    let bytes = ipc::read_frame(&mut stream, Duration::from_secs(5)).await?;
    let _peer = stream.authenticate_after_read()?;
    let request: Request = wire::decode("Request", &bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    if request.method != Method::Hello {
        send(&mut stream, Response::failure(request.request_id, WireErrorCode::HandshakeRequired)).await?;
        return Ok(());
    }
    let id = request.request_id;
    let hello = serde_json::from_value(request.params).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    let (session, hello) = match host.hello(hello) {
        Ok(accepted) => accepted,
        Err(code) => { send(&mut stream, Response::failure(id, code)).await?; return Ok(()); },
    };
    send(&mut stream, Response::success(id, serde_json::to_value(hello)?)).await?;
    requests(host, session, stream, dispatch_slots).await
}

async fn requests(host: Arc<Host>, session: Session, mut stream: Stream, dispatch_slots: Arc<Semaphore>) -> io::Result<()> {
    let session = Arc::new(session);
    loop {
        let bytes = ipc::read_frame(&mut stream, Duration::from_secs(30)).await?;
        let request: Request = wire::decode("Request", &bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let permit = match tokio::time::timeout(Duration::from_secs(5), dispatch_slots.clone().acquire_owned()).await {
            Ok(Ok(permit)) => permit,
            _ => { send(&mut stream, Response::failure(request.request_id, WireErrorCode::Busy)).await?; continue; },
        };
        let worker_host = host.clone();
        let worker_session = session.clone();
        let response = tokio::task::spawn_blocking(move || { let _permit = permit; worker_host.dispatch(&worker_session, request) }).await.map_err(|_| io::Error::from(io::ErrorKind::Other))?;
        send(&mut stream, response).await?;
        if host.is_stopping() && host.capture_stopped() { return Ok(()); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Client, ControlFile};
    use dzc_protocol::wire::*;
    #[test]
    fn real_local_client_requires_explicit_grants_and_revocation_applies_to_live_connections() {
        let directory = std::env::temp_dir().join(format!("dzc-host-native-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        #[cfg(target_os="linux")]
        { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap(); }
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let host = crate::router::tests::synthetic_host();
            let endpoint = Endpoint::in_directory(&directory).unwrap();
            let listener = endpoint.bind().unwrap();
            let credentials = ControlFile::publish(&host, &endpoint).unwrap();
            let (_shutdown, receiver) = watch::channel(false);
            let server = tokio::spawn(serve_listener(host.clone(), listener, receiver));
            let mut untrusted = Client::connect(&endpoint, ClientRole::Desktop, None).await.unwrap();
            assert_eq!(untrusted.peer.pid(), Some(std::process::id()));
            let denied: io::Result<PrivacyState> = untrusted.call(Method::PrivacySet, serde_json::json!({"active":true}), "PrivacyState").await;
            assert!(denied.is_err());
            let mut control = Client::control(&endpoint, ClientRole::Cli).await.unwrap();
            let grant: GrantResult = control.call(Method::ServiceGrant, serde_json::json!({"role":"surface","permissions":["clipboard_copy"],"lifetime_ms":30000}), "GrantResult").await.unwrap();
            let mut surface = Client::connect(&endpoint, ClientRole::Surface, Some(grant.grant.clone())).await.unwrap();
            assert!(!surface.hello.permissions.contains(&Permission::HistoryBody));
            let invalid: io::Result<GrantResult> = control.call(Method::ServiceGrant, serde_json::json!({"role":"surface","permissions":["history_body"],"lifetime_ms":30000}), "GrantResult").await;
            assert!(invalid.is_err());
            let _: OperationResult = control.call(Method::ServiceRevoke, serde_json::json!({"grant":grant.grant}), "OperationResult").await.unwrap();
            let denied: io::Result<ServiceStatus> = surface.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await;
            assert!(denied.is_err(), "revocation must apply without reconnect");
            let _: OperationResult = control.call(Method::ServiceStop, serde_json::json!({}), "OperationResult").await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), server).await.unwrap().unwrap().unwrap();
            drop(credentials);
            assert!(!endpoint.control_path().exists());
        });
        std::fs::remove_dir_all(directory).unwrap();
    }
}
