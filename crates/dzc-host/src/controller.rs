//! No auto-launch: callers explicitly request every service transition.
use crate::{Client, data};
use dzc_platform::ipc::Endpoint;
use dzc_protocol::{build_info::{BuildInfo, Component}, wire::*};
use std::{ffi::OsString, io, path::{Path, PathBuf}, time::Duration};
struct UnconfirmedChild { child: dzc_platform::ipc::BackgroundChild, acknowledged: bool }
impl Drop for UnconfirmedChild { fn drop(&mut self) { if !self.acknowledged { let _=self.child.kill(); let _=self.child.wait(); } } }

pub fn endpoint(database: &Path) -> io::Result<Endpoint> {
    let directory = data::canonical_directory(database).map_err(io::Error::other)?;
    Endpoint::for_current_session(&directory)
}
pub fn sibling_executable() -> io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    let extension = current.extension().map(|value| format!(".{}", value.to_string_lossy())).unwrap_or_default();
    Ok(current.with_file_name(format!("dzc-service{extension}")))
}
pub async fn status(database: &Path) -> io::Result<ServiceStatus> {
    let mut client = Client::connect(&endpoint(database)?, ClientRole::Cli, None).await?;
    client.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await
}
pub async fn stop(database: &Path) -> io::Result<()> {
    let endpoint = endpoint(database)?;
    let mut client = Client::control(&endpoint, ClientRole::Cli).await?;
    let status: ServiceStatus = client.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
    let process = if status.mode == HostMode::Service { Some(dzc_platform::ipc::ProcessExit::open(&client.peer)?) } else { None };
    let _: OperationResult = client.call(Method::ServiceStop, serde_json::json!({}), "OperationResult").await?;
    // Ownership is acknowledged before the response. Confirm that the old
    // instance's listener has also exited rather than claiming stop on send.
    let old_instance = client.hello.state.service_instance_id.clone();
    drop(client);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match Client::connect(&endpoint, ClientRole::Cli, None).await {
            Err(_) => break,
            Ok(client) if client.hello.state.service_instance_id != old_instance => break,
            Ok(_) => (),
        }
        if tokio::time::Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "HostStopUnconfirmed")); }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if let Some(process) = process {
        tokio::task::spawn_blocking(move || process.wait(Duration::from_secs(5))).await.map_err(io::Error::other)??;
    }
    Ok(())
}
pub async fn start(database: &Path, executable: &Path, no_capture: bool) -> io::Result<ServiceStatus> {
    start_preflight(database,executable,no_capture,dzc_platform::ipc::ProcessExit::ensure_supported).await
}
async fn start_preflight(database: &Path, executable: &Path, no_capture: bool, probe: impl FnOnce()->io::Result<()>) -> io::Result<ServiceStatus> {
    // This must precede even endpoint preparation or helper inspection. An
    // unsupported service transition leaves the existing embedded owner alone.
    probe()?;
    let endpoint = endpoint(database)?;
    // The explicit helper must share the executable contract before startup.
    // This query neither opens history nor initializes native clipboard ports.
    let output = dzc_platform::ipc::inspect_helper(executable, &["--build-info"], Duration::from_secs(5), 16_384)?;
    let identity: BuildInfo = serde_json::from_slice(&output)?;
    let current = BuildInfo::current_with_host(Component::Cli, dzc_core::api::DATABASE_SCHEMA_VERSION);
    if identity.component != Component::Service || identity.component_version != current.component_version
        || identity.protocol_version != current.protocol_version || identity.database_schema_version != current.database_schema_version
        || (current.source_state == "verified_checkout" && !current.same_verified_build_set(&identity)) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "ServiceBuildSetMismatch"));
    }
    if let Ok(mut existing) = Client::control(&endpoint, ClientRole::Cli).await {
        drop(dzc_platform::ipc::ProcessExit::open(&existing.peer)?);
        let status: ServiceStatus = existing.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
        if status.mode == HostMode::Service {
            if status.capture_owner != !no_capture { return Err(io::Error::new(io::ErrorKind::AlreadyExists,"ServiceCaptureModeConflict")); }
            return Ok(status);
        }
        let _: OperationResult = existing.call(Method::ServiceHandoff, serde_json::json!({}), "OperationResult").await?;
        let previous = existing.hello.state.service_instance_id.clone();
        drop(existing);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            match Client::connect(&endpoint, ClientRole::Cli, None).await {
                Err(_) => break,
                Ok(client) if client.hello.state.service_instance_id != previous => return Err(io::Error::new(io::ErrorKind::AlreadyExists,"HostChangedDuringHandoff")),
                _ => (),
            }
            if tokio::time::Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut,"HostHandoffUnconfirmed")); }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    let absolute_database = if database.is_absolute() { database.to_owned() } else { std::env::current_dir()?.join(database) };
    let mut arguments = vec![OsString::from("--database"), absolute_database.into_os_string()];
    if no_capture { arguments.push(OsString::from("--no-capture")); }
    let mut child = UnconfirmedChild { child: dzc_platform::ipc::spawn_background(executable, &arguments)?, acknowledged:false };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if child.child.try_wait()?.is_some() { return Err(io::Error::new(io::ErrorKind::Other, "ServiceStartupFailed")); }
        if let Ok(mut client) = Client::control(&endpoint, ClientRole::Cli).await {
            if client.peer.pid() == Some(child.child.id()) {
                let status: ServiceStatus = client.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
                if status.mode == HostMode::Service && (no_capture || status.capture_owner) { child.acknowledged=true; return Ok(status); }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            // Only the exact child spawned above is terminated. Kernel capture
            // ownership is released after all of that process's workers exit.
            return Err(io::Error::new(io::ErrorKind::TimedOut, "ServiceStartupUnconfirmed"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Host, HostOptions, HostLifecycle, ControlFile};
    use std::sync::{Arc, atomic::{AtomicUsize,Ordering}};
    struct StopCount(Arc<AtomicUsize>);
    impl HostLifecycle for StopCount {
        fn stop_capture(&self)->Result<(),WireErrorCode> { self.0.fetch_add(1,Ordering::SeqCst); Ok(()) }
    }
    #[test]
    fn unsupported_process_observation_leaves_a_real_embedded_host_untouched() {
        let directory=std::env::temp_dir().join(format!("dzc-service-preflight-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        #[cfg(target_os="linux")]
        { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&directory,std::fs::Permissions::from_mode(0o700)).unwrap(); }
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let fixture=crate::router::tests::synthetic_host();
            let host=Host::new(HostOptions {mode:HostMode::Embedded,history:fixture.history.clone(),directory:directory.clone(),actions:Arc::new(crate::router::tests::Actions::default())}).unwrap();
            let stopped=Arc::new(AtomicUsize::new(0));
            host.attach_lifecycle(Arc::new(StopCount(stopped.clone()))).unwrap();
            let database=directory.join("not-opened.db");
            let endpoint=endpoint(&database).unwrap();
            let listener=endpoint.bind().unwrap();
            let credentials=ControlFile::publish(&host,&endpoint).unwrap();
            let (_shutdown,receiver)=tokio::sync::watch::channel(false);
            let server=tokio::spawn(crate::serve_listener(host.clone(),listener,receiver));
            let original=status(&database).await.unwrap();
            let error=start_preflight(&database,&directory.join("nonexistent-service"),false,||Err(io::Error::new(io::ErrorKind::Unsupported,"SyntheticProcessObservationDenied"))).await.unwrap_err();
            assert_eq!(error.kind(),io::ErrorKind::Unsupported);
            assert_eq!(status(&database).await.unwrap().state.service_instance_id,original.state.service_instance_id);
            assert_eq!(stopped.load(Ordering::SeqCst),0);
            assert!(!host.is_stopping());
            assert!(!database.exists());
            stop(&database).await.unwrap();
            server.await.unwrap().unwrap();
            assert_eq!(stopped.load(Ordering::SeqCst),1);
            drop(credentials);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }
}
