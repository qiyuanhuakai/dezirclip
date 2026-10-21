use crate::{Host, Session};
use dzc_platform::ipc::{self, Endpoint, Stream};
use dzc_protocol::wire::{self, Method, Request, Response, WireErrorCode};
use std::{io, sync::Arc, time::Duration};
use tokio::{sync::{watch, Semaphore}, task::JoinSet};

struct Connections {
    draining: watch::Sender<bool>,
    dispatch_slots: Arc<Semaphore>,
    #[cfg(test)]
    before_ack: std::sync::Mutex<Option<(tokio::sync::oneshot::Sender<()>, tokio::sync::oneshot::Receiver<()>)>>,
    #[cfg(test)]
    finished: std::sync::atomic::AtomicUsize,
}
impl Connections {
    fn new() -> Arc<Self> {
        let (draining, _) = watch::channel(false);
        Arc::new(Self { draining, dispatch_slots: Arc::new(Semaphore::new(1)),
            #[cfg(test)] before_ack: std::sync::Mutex::new(None),
            #[cfg(test)] finished: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    async fn before_response(&self, _method: Method) {
        #[cfg(test)]
        if matches!(_method, Method::ServiceStop | Method::ServiceHandoff) {
            let barrier = self.before_ack.lock().unwrap().take();
            if let Some((reached, resume)) = barrier { let _ = reached.send(()); let _ = resume.await; }
        }
    }
}

/// The caller owns the runtime; no process or worker is started by constructing
/// a Host. Start this future only when embedded IPC or service mode is enabled.
pub async fn serve(host: Arc<Host>, endpoint: Endpoint, shutdown: watch::Receiver<bool>) -> io::Result<()> {
    serve_listener(host, endpoint.bind()?, shutdown).await
}
pub async fn serve_listener(host: Arc<Host>, listener: ipc::Listener, shutdown: watch::Receiver<bool>) -> io::Result<()> {
    serve_connections(host, listener, shutdown, Connections::new()).await
}
async fn serve_connections(host: Arc<Host>, mut listener: ipc::Listener, mut shutdown: watch::Receiver<bool>, state: Arc<Connections>) -> io::Result<()> {
    // NamedPipe reserves an additional listening instance below its kernel cap.
    let connections = Arc::new(Semaphore::new(16));
    let mut draining = state.draining.subscribe();
    let mut workers = JoinSet::new();
    let mut accept_error = None;
    loop {
        if *shutdown.borrow() || *draining.borrow() || host.is_stopping() && host.capture_stopped() { break; }
        let permit = tokio::select! {
            _ = draining.changed() => break,
            changed = shutdown.changed() => { if changed.is_err() || *shutdown.borrow() { break; } else { continue; } },
            permit = connections.clone().acquire_owned() => permit.map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?,
            Some(_) = workers.join_next(), if !workers.is_empty() => continue,
        };
        let stream = tokio::select! {
            _ = draining.changed() => break,
            changed = shutdown.changed() => { if changed.is_err() || *shutdown.borrow() { break; } else { continue; } },
            stream = listener.accept() => match stream { Ok(stream) => stream, Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue, Err(error) => { accept_error = Some(error); break; } },
            Some(_) = workers.join_next(), if !workers.is_empty() => continue,
        };
        let host = host.clone();
        let state = state.clone();
        workers.spawn(async move {
            let _permit = permit;
            let _ = connection(host, stream, state.clone()).await;
            #[cfg(test)] state.finished.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
    }
    drop(listener);
    let _ = state.draining.send(true);
    // Cancel only idle/partial frame reads. Every fully admitted request keeps
    // its dispatch and bounded response write until completion, including ACKs.
    while workers.join_next().await.is_some() {}
    if let Some(error) = accept_error { Err(error) } else { Ok(()) }
}

async fn send(stream: &mut Stream, response: Response) -> io::Result<()> {
    let bytes = wire::encode("Response", &response).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    ipc::write_frame(stream, &bytes, Duration::from_secs(5)).await
}

async fn read_request(stream: &mut Stream, stopped: &mut watch::Receiver<bool>, timeout: Duration) -> io::Result<Vec<u8>> {
    if *stopped.borrow() { return Err(io::Error::from(io::ErrorKind::Interrupted)); }
    tokio::select! {
        _ = stopped.changed() => Err(io::Error::from(io::ErrorKind::Interrupted)),
        frame = ipc::read_frame(stream, timeout) => frame,
    }
}

async fn connection(host: Arc<Host>, mut stream: Stream, state: Arc<Connections>) -> io::Result<()> {
    let mut stopped = state.draining.subscribe();
    let bytes = read_request(&mut stream, &mut stopped, Duration::from_secs(5)).await?;
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
    requests(host, session, stream, state, stopped).await
}

async fn requests(host: Arc<Host>, session: Session, mut stream: Stream, state: Arc<Connections>, mut stopped: watch::Receiver<bool>) -> io::Result<()> {
    let session = Arc::new(session);
    loop {
        let bytes = read_request(&mut stream, &mut stopped, Duration::from_secs(30)).await?;
        let request: Request = wire::decode("Request", &bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let method = request.method;
        let permit = if method == Method::EventsSubscribe { None } else {
            match tokio::time::timeout(Duration::from_secs(5), state.dispatch_slots.clone().acquire_owned()).await {
                Ok(Ok(permit)) => Some(permit),
                _ => { send(&mut stream, Response::failure(request.request_id, WireErrorCode::Busy)).await?; continue; },
            }
        };
        let worker_host = host.clone();
        let worker_session = session.clone();
        let response = tokio::task::spawn_blocking(move || { let _permit = permit; worker_host.dispatch(&worker_session, request) }).await.map_err(|_| io::Error::from(io::ErrorKind::Other))?;
        if host.is_stopping() && host.capture_stopped() { let _ = state.draining.send(true); }
        state.before_response(method).await;
        send(&mut stream, response).await?;
        if host.is_stopping() && host.capture_stopped() { return Ok(()); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Client, ControlFile};
    use dzc_protocol::wire::*;
    fn directory() -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("dzc-host-drain-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        #[cfg(target_os="linux")]
        { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap(); }
        directory
    }

    #[test]
    fn concurrent_native_subscriptions_do_not_queue_privacy_revocation_or_stop() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            for operation in [Method::PrivacySet, Method::ServiceRevoke, Method::ServiceStop] {
                let directory = directory();
                let host = crate::router::tests::synthetic_host();
                let endpoint = Endpoint::in_directory(&directory).unwrap();
                let listener = endpoint.bind().unwrap();
                let credentials = ControlFile::publish(&host, &endpoint).unwrap();
                let (_shutdown, receiver) = watch::channel(false);
                let server = tokio::spawn(serve_listener(host.clone(), listener, receiver));
                let mut control = Client::control(&endpoint, ClientRole::Cli).await.unwrap();
                let grant = host.grants().issue(ClientRole::Mcp, vec![Permission::HistoryRead], Duration::from_secs(30)).unwrap();
                let mut waiters = Vec::new();
                for _ in 0..6 {
                    let mut client = Client::connect(&endpoint, ClientRole::Mcp, Some(grant.token().into())).await.unwrap();
                    waiters.push(tokio::spawn(async move { client.call::<EventsResult>(Method::EventsSubscribe,
                        serde_json::json!({"after_sequence":0,"wait_ms":1000}), "EventsResult").await }));
                }
                tokio::time::timeout(Duration::from_millis(800), async {
                    while host.waiting_subscriptions() != 6 { tokio::time::sleep(Duration::from_millis(1)).await; }
                }).await.expect("six independent waits must coexist before mutation");
                let parameters = match operation {
                    Method::PrivacySet => serde_json::json!({"active":true}),
                    Method::ServiceRevoke => serde_json::json!({"grant":grant.token()}),
                    _ => serde_json::json!({}),
                };
                if operation == Method::PrivacySet {
                    let result = tokio::time::timeout(Duration::from_millis(500), control.call::<PrivacyState>(operation, parameters, "PrivacyState")).await.unwrap().unwrap();
                    assert!(result.active);
                } else {
                    let result = tokio::time::timeout(Duration::from_millis(500), control.call::<OperationResult>(operation, parameters, "OperationResult")).await.unwrap().unwrap();
                    assert!(result.performed);
                }
                for waiter in waiters {
                    let result = tokio::time::timeout(Duration::from_millis(500), waiter).await.unwrap().unwrap();
                    if operation == Method::PrivacySet {
                        let result = result.unwrap();
                        assert!(result.events.iter().any(|event| event.event == EventKind::PrivacyStateChanged));
                        assert_eq!(result.state.privacy_epoch, host.snapshot().unwrap().privacy_epoch);
                    } else { assert!(result.is_err(), "revoked/stopped subscriptions must not return stale success"); }
                }
                if operation != Method::ServiceStop { let _: OperationResult = control.call(Method::ServiceStop, serde_json::json!({}), "OperationResult").await.unwrap(); }
                tokio::time::timeout(Duration::from_secs(2), server).await.unwrap().unwrap().unwrap();
                drop(credentials);
                std::fs::remove_dir_all(directory).unwrap();
            }
        });
    }

    #[test]
    fn admitted_stop_and_handoff_ack_survive_another_connection_finishing_during_drain() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            for method in [Method::ServiceStop, Method::ServiceHandoff] {
                let directory = directory();
                let host = crate::router::tests::synthetic_host();
                let endpoint = Endpoint::in_directory(&directory).unwrap();
                let listener = endpoint.bind().unwrap();
                let credentials = ControlFile::publish(&host, &endpoint).unwrap();
                let state = Connections::new();
                let (reached, at_ack) = tokio::sync::oneshot::channel();
                let (resume, resumed) = tokio::sync::oneshot::channel();
                *state.before_ack.lock().unwrap() = Some((reached, resumed));
                let (_shutdown, receiver) = watch::channel(false);
                let server = tokio::spawn(serve_connections(host.clone(), listener, receiver, state.clone()));
                let mut a = Client::control(&endpoint, ClientRole::Cli).await.unwrap();
                let b = Client::connect(&endpoint, ClientRole::Surface, None).await.unwrap();
                let ack = tokio::spawn(async move { a.call::<OperationResult>(method, serde_json::json!({}), "OperationResult").await });
                tokio::time::timeout(Duration::from_secs(2), at_ack).await.unwrap().unwrap();
                assert!(host.capture_stopped());
                drop(b);
                tokio::time::timeout(Duration::from_secs(2), async {
                    while state.finished.load(std::sync::atomic::Ordering::SeqCst) != 1 { tokio::task::yield_now().await; }
                }).await.expect("the other native connection must finish while A is paused");
                assert!(!server.is_finished(), "the admitted ACK must still own its connection");
                assert!(!ack.is_finished());
                resume.send(()).unwrap();
                assert!(tokio::time::timeout(Duration::from_secs(2), ack).await.unwrap().unwrap().unwrap().performed);
                tokio::time::timeout(Duration::from_secs(2), server).await.unwrap().unwrap().unwrap();
                assert!(endpoint.connect(Duration::from_millis(100)).await.is_err());
                drop(credentials);
                std::fs::remove_dir_all(directory).unwrap();
            }
        });
    }
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
