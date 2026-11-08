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
/// Reserve both session endpoints until the new profile and its redirect are published.
pub fn move_profile_offline(database: &Path, destination: &Path) -> io::Result<()> {
    let (source, target) = data::installed_move_directories(database, destination)?;
    if source == target { return Ok(()); }
    let _source = data::reserve_profile(&source)?;
    let _destination = data::reserve_profile(&target)?;
    let redirect = data::move_redirect(database)?;
    let history = dzc_core::api::HistoryService::open(database).map_err(io::Error::other)?;
    let original_directory = database.parent().ok_or_else(|| io::Error::other("Missing profile directory"))?;
    history.migrate_profile(original_directory, destination, &redirect).map_err(io::Error::other)
}
/// Move the installed profile through its original authenticated Service. A copy failure leaves
/// that Service running. Only a confirmed cutover permits starting the Service at the new path.
#[derive(serde::Serialize)]
pub struct MoveRecovery {
    previous_directory: PathBuf,
    target: PathBuf,
    instance: String,
    no_capture: bool,
    pub detail: String,
}

pub async fn move_active_profile(database: &Path, destination: &Path, instance: &str, epoch: u64) -> io::Result<Option<MoveRecovery>> {
    let (source, target) = data::installed_move_directories(database, destination)?;
    let sibling = sibling_executable()?;
    let old_endpoint = endpoint(database)?;
    let mut client = Client::control(&old_endpoint, ClientRole::Desktop).await?;
    if client.hello.state.service_instance_id != instance {
        return Err(io::Error::other("The desktop Service changed; reopen Settings"));
    }
    let before: ServiceStatus = client.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
    if before.mode != HostMode::Service { return Err(io::Error::other("Close the embedded desktop Host before moving its profile")); }
    if before.state.privacy_epoch != epoch || before.privacy.active {
        return Err(io::Error::other("The profile privacy state changed; reopen Settings"));
    }
    let previous = dzc_platform::ipc::ProcessExit::open(&client.peer)?;
    let path = destination.to_str().ok_or_else(|| io::Error::other("Invalid destination"))?;
    let moved: ProfileMoveResult = client.call(Method::DesktopProfileMove, BackupPreviewParams {
        service_instance_id: instance.to_owned(), expected_epoch: epoch, path: path.to_owned(),
    }, "ProfileMoveResult").await?;
    if !moved.moved { return Ok(None); }
    let no_capture = !before.capture_owner;
    let resume = async {
        if !moved.service_stopped {
            let _: OperationResult = client.call(Method::ServiceStop, serde_json::json!({}), "OperationResult").await
                .map_err(|_| io::Error::other("Data moved, but the old Service did not confirm shutdown"))?;
        }
        drop(client);
        tokio::task::spawn_blocking(move || previous.wait(Duration::from_secs(10))).await
            .map_err(io::Error::other).and_then(|result| result)
            .map_err(|_| io::Error::other("Data moved, but the old Service process did not confirm exit"))?;
        restart_moved_profile(&target, &sibling, instance, no_capture).await
    }.await;
    Ok(resume.err().map(|error| MoveRecovery {
        previous_directory: source, target, instance: instance.to_owned(), no_capture, detail: error.to_string(),
    }))
}

/// Resume only the cutover. Validate the installed redirect before touching either endpoint.
pub async fn recover_profile(target: &Path, previous: &Path, instance: &str, no_capture: bool) -> io::Result<()> {
    let selected = data::database_path().map_err(io::Error::other)?;
    let (current, old) = data::installed_move_directories(&selected, previous)?;
    if current != target.canonicalize()? || current == old || instance.is_empty() {
        return Err(io::Error::other("The moved profile is no longer selected"));
    }
    let endpoint = endpoint(&old.join("clipboard.db"))?;
    match Client::connect(&endpoint, ClientRole::Desktop, None).await {
        Ok(probe) => {
            drop(probe);
            let mut client = Client::control(&endpoint, ClientRole::Desktop).await?;
            let status: ServiceStatus = client.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
            if status.mode != HostMode::Service || status.state.service_instance_id != instance {
                return Err(io::Error::other("A different Host now owns the previous profile"));
            }
            let previous = dzc_platform::ipc::ProcessExit::open(&client.peer)?;
            let _: OperationResult = client.call(Method::ServiceStop, serde_json::json!({}), "OperationResult").await?;
            drop(client);
            tokio::task::spawn_blocking(move || previous.wait(Duration::from_secs(10))).await.map_err(io::Error::other)??;
        }
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => (),
        Err(error) => return Err(error),
    }
    let _old = data::reserve_profile(&old)?;
    restart_moved_profile(&current, &sibling_executable()?, instance, no_capture).await
}

async fn restart_moved_profile(target: &Path, sibling: &Path, instance: &str, no_capture: bool) -> io::Result<()> {
    let selected = data::database_path().map_err(|_| io::Error::other("Data moved, but the new profile could not be located"))?;
    let actual = selected.canonicalize().map_err(|_| io::Error::other("Data moved, but the selected database is unavailable"))?;
    let expected = target.join("clipboard.db").canonicalize().map_err(|_| io::Error::other("Data moved, but the destination database is unavailable"))?;
    if actual != expected {
        return Err(io::Error::other("Data moved, but the selected profile changed before restart"));
    }
    let after = start_preflight(&selected, sibling, no_capture, false, dzc_platform::ipc::ProcessExit::ensure_supported).await
        .map_err(|_| io::Error::other("Data moved, but the replacement Service could not start"))?;
    if after.mode != HostMode::Service || after.state.service_instance_id == instance || after.capture_owner == no_capture {
        return Err(io::Error::other("Data moved, but the replacement Service has an unexpected owner or capture mode"));
    }
    Ok(())
}
pub async fn status(database: &Path) -> io::Result<ServiceStatus> {
    let mut client = Client::connect(&endpoint(database)?, ClientRole::Cli, None).await?;
    client.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await
}
pub async fn stop(database: &Path) -> io::Result<()> {
    stop_expected(database, None).await
}

/// Validate the observed instance on the same authenticated connection used to
/// stop it. A desktop handoff must never stop a replacement Host found later.
pub async fn stop_instance(database: &Path, instance: &str) -> io::Result<()> {
    stop_expected(database, Some(instance)).await
}

async fn stop_expected(database: &Path, instance: Option<&str>) -> io::Result<()> {
    let endpoint = endpoint(database)?;
    let mut client = Client::control(&endpoint, ClientRole::Cli).await?;
    if instance.is_some_and(|expected| expected != client.hello.state.service_instance_id) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "HostChangedBeforeStop"));
    }
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
    start_preflight(database,executable,no_capture,true,dzc_platform::ipc::ProcessExit::ensure_supported).await
}

/// Invoked by the elevated desktop helper after Windows has accepted UAC.
/// Validate the packaged Service before stopping the observed instance, and
/// preserve whether that instance owns clipboard capture.
pub async fn restart_elevated(database: &Path, executable: &Path, instance: &str) -> io::Result<ServiceStatus> {
    if !dzc_platform::desktop_system::supported() || !dzc_platform::desktop_system::is_admin()? {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "ElevatedWindowsProcessRequired"));
    }
    let original = status(database).await?;
    if original.mode != HostMode::Service || original.state.service_instance_id != instance {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "ServiceChangedBeforeElevation"));
    }
    dzc_platform::ipc::ProcessExit::ensure_supported()?;
    inspect_service_build(executable)?;
    stop_instance(database, instance).await?;
    let replacement = start_verified(database, executable, !original.capture_owner, false, true, endpoint(database)?).await?;
    if replacement.mode != HostMode::Service || replacement.state.service_instance_id == instance || replacement.capture_owner != original.capture_owner {
        return Err(io::Error::other("ElevatedServiceReplacementUnconfirmed"));
    }
    Ok(replacement)
}
/// Find the current desktop Host. Starting the sibling Service is opt-in and
/// cannot displace an Embedded Host that wins a concurrent startup.
pub async fn desktop_endpoint(database: &Path, start_if_missing: bool) -> io::Result<serde_json::Value> {
    let endpoint = endpoint(database)?;
    match Client::connect(&endpoint, ClientRole::Desktop, None).await {
        Ok(_) => (),
        Err(error) if start_if_missing && matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
            if let Err(start_error) = start_preflight(database, &sibling_executable()?, false, false,
                dzc_platform::ipc::ProcessExit::ensure_supported).await {
                // Concurrent starts may lose the exclusive bind. Accept only a
                // responsive owner, authenticated again below.
                if Client::connect(&endpoint, ClientRole::Desktop, None).await.is_err() {
                    return Err(start_error);
                }
            }
        }
        Err(error) => return Err(error),
    }
    let client = Client::control(&endpoint, ClientRole::Desktop).await?;
    if !client.hello.permissions.contains(&Permission::DesktopHistory) {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    Ok(serde_json::json!({
        "address": endpoint.address(), "control_path": endpoint.control_path(),
        "pid": client.peer.pid().ok_or_else(|| io::Error::other("HostPidUnavailable"))?,
        "instance": client.hello.state.service_instance_id,
        "version": client.hello.core_version,
    }))
}

async fn start_preflight(database: &Path, executable: &Path, no_capture: bool, allow_handoff: bool, probe: impl FnOnce()->io::Result<()>) -> io::Result<ServiceStatus> {
    // This must precede even endpoint preparation or helper inspection. An
    // unsupported service transition leaves the existing embedded owner alone.
    probe()?;
    let endpoint = endpoint(database)?;
    inspect_service_build(executable)?;
    start_verified(database, executable, no_capture, allow_handoff, false, endpoint).await
}

fn inspect_service_build(executable: &Path) -> io::Result<()> {
    // This query neither opens history nor initializes native clipboard ports.
    let output = dzc_platform::ipc::inspect_helper(executable, &["--build-info"], Duration::from_secs(5), 16_384)?;
    let identity: BuildInfo = serde_json::from_slice(&output)?;
    let current = BuildInfo::current_with_host(Component::Cli, dzc_core::api::DATABASE_SCHEMA_VERSION);
    if identity.component != Component::Service || identity.component_version != current.component_version
        || identity.protocol_version != current.protocol_version || identity.database_schema_version != current.database_schema_version
        || (current.source_state == "verified_checkout" && !current.same_verified_build_set(&identity)) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "ServiceBuildSetMismatch"));
    }
    Ok(())
}

async fn start_verified(database: &Path, executable: &Path, no_capture: bool, allow_handoff: bool, require_new: bool, endpoint: Endpoint) -> io::Result<ServiceStatus> {
    if let Ok(mut existing) = Client::control(&endpoint, ClientRole::Cli).await {
        if require_new { return Err(io::Error::new(io::ErrorKind::AlreadyExists, "AnotherHostStartedDuringElevation")); }
        drop(dzc_platform::ipc::ProcessExit::open(&existing.peer)?);
        let status: ServiceStatus = existing.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
        if !allow_handoff { return Ok(status); }
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
            let wrong_instance=uuid::Uuid::new_v4().to_string();
            let rejected=stop_instance(&database,&wrong_instance).await.unwrap_err();
            assert_eq!(rejected.kind(),io::ErrorKind::InvalidInput);
            assert_eq!(status(&database).await.unwrap().state.service_instance_id,original.state.service_instance_id);
            assert_eq!(stopped.load(Ordering::SeqCst),0);
            assert!(!host.is_stopping());
            let replacement=start_verified(&database,&directory.join("nonexistent-service"),true,false,true,endpoint.clone()).await.unwrap_err();
            assert_eq!(replacement.kind(),io::ErrorKind::AlreadyExists);
            assert_eq!(stopped.load(Ordering::SeqCst),0);
            assert!(!host.is_stopping());
            let error=start_preflight(&database,&directory.join("nonexistent-service"),false,true,||Err(io::Error::new(io::ErrorKind::Unsupported,"SyntheticProcessObservationDenied"))).await.unwrap_err();
            assert_eq!(error.kind(),io::ErrorKind::Unsupported);
            assert_eq!(status(&database).await.unwrap().state.service_instance_id,original.state.service_instance_id);
            assert_eq!(stopped.load(Ordering::SeqCst),0);
            assert!(!host.is_stopping());
            assert!(!database.exists());
            stop_instance(&database,&original.state.service_instance_id).await.unwrap();
            server.await.unwrap().unwrap();
            assert_eq!(stopped.load(Ordering::SeqCst),1);
            drop(credentials);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }
}
