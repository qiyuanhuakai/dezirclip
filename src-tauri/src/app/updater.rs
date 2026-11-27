use dzc_host::update::{automatic_check_due, expected_asset_name, summarize_channel_versions, validate_signature_metadata, AvailableUpdate, UpdateChannel, UpdateCheck, UpdateSupport, RELEASES_URL};
use dzc_core::api::UpdateAttemptChannel;
use crate::infrastructure::update_support::installation_support;
use semver::Version;
use serde::Serialize;
use std::time::Duration;
use tauri::{ipc::Channel, AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;
use std::sync::Arc;
use crate::domain::update_host::{UpdateHost, NoAuxiliaryClients, install_after_host_ready};

pub struct UpdaterState {
    pending: Mutex<Option<PendingUpdate>>,
    host: Arc<dyn UpdateHost>,
    last_check: std::sync::Mutex<Option<CompletedCheck>>,
    preferences_changed: tokio::sync::Notify,
}

impl Default for UpdaterState {
    fn default() -> Self { Self::with_host(Arc::new(NoAuxiliaryClients)) }
}

impl UpdaterState {
    /// Future Host adapters supply normal-exit coordination for owned clients.
    pub fn with_host(host: Arc<dyn UpdateHost>) -> Self {
        Self { pending: Mutex::new(None), host, last_check: Default::default(), preferences_changed: Default::default() }
    }

    pub fn preferences_changed(&self) {
        self.preferences_changed.notify_one();
    }
}

struct PendingUpdate {
    id: String,
    update: Update,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompletedCheck {
    channel: UpdateChannel,
    checked_at: i64,
    result: Result<UpdateCheck, String>,
}

const AUTOMATIC_CHECK_DELAY: Duration = Duration::from_secs(15);
const AUTOMATIC_POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);

pub fn start_automatic_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<UpdaterState>();
        let mut delay = AUTOMATIC_CHECK_DELAY;
        loop {
            tokio::select! {
                _ = state.preferences_changed.notified() => { delay = AUTOMATIC_CHECK_DELAY; continue; }
                _ = tokio::time::sleep(delay) => {}
            }
            delay = AUTOMATIC_POLL_INTERVAL;
            let database = app.state::<crate::database::DbState>();
            let Ok(Some(preference)) = database.history_service().automatic_update_preference() else {
                continue;
            };
            let channel = UpdateChannel::from_preference(preference.saved_channel.as_deref(), &app.package_info().version);
            if let Ok(Some(check)) = run_check(&app, &state, channel, true).await {
                let _ = app.emit_to("main", "update-check-completed", check);
            }
        }
    });
}

#[tauri::command]
pub(crate) fn get_completed_update_check(state: State<'_, UpdaterState>, channel: UpdateChannel) -> Option<CompletedCheck> {
    state.last_check.lock().ok()?.as_ref().filter(|check| check.channel == channel).cloned()
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum UpdateProgress {
    Downloading { downloaded: u64, total: Option<u64> },
    Verified,
    Installing,
}

#[tauri::command]
pub fn get_update_support() -> UpdateSupport {
    installation_support()
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    channel: UpdateChannel,
) -> Result<UpdateCheck, String> {
    run_check(&app, &state, channel, false).await?
        .ok_or("update_check_required")?.result
}

async fn run_check(app: &AppHandle, state: &UpdaterState, channel: UpdateChannel, automatic: bool)
    -> Result<Option<CompletedCheck>, String>
{
    let mut pending = state.pending.try_lock().map_err(|_| "update_busy")?;
    let database = app.state::<crate::database::DbState>();
    let history = database.history_service();
    let receipt_channel = match channel {
        UpdateChannel::Stable => UpdateAttemptChannel::Stable,
        UpdateChannel::Preview => UpdateAttemptChannel::Preview,
    };
    let now = chrono::Utc::now().timestamp_millis();
    if automatic {
        let previous = history.last_update_attempt(receipt_channel).map_err(|_| "update_attempt_storage_failed")?;
        if !automatic_check_due(previous.as_deref(), now) {
            return Ok(None);
        }
    }
    let recorded = history.record_update_attempt(receipt_channel, now);
    *pending = None;
    let result = if automatic && recorded.is_err() {
        Err("update_attempt_storage_failed".into())
    } else {
        fetch_update(app, channel, &mut pending).await
    };
    let check = CompletedCheck { channel, checked_at: chrono::Utc::now().timestamp_millis(), result };
    *state.last_check.lock().map_err(|_| "update_unavailable")? = Some(check.clone());
    Ok(Some(check))
}

async fn fetch_update(app: &AppHandle, channel: UpdateChannel, pending: &mut Option<PendingUpdate>) -> Result<UpdateCheck, String> {
    let configured_key = app.config().plugins.0.get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(|value| value.as_str()).unwrap_or_default();
    if configured_key.trim().is_empty() {
        return Err("update_signing_not_configured".into());
    }
    let endpoint = channel.endpoint().parse().map_err(|_| "update_invalid_endpoint")?;
    let updater = app.updater_builder()
        .endpoints(vec![endpoint]).map_err(|_| "update_invalid_endpoint")?
        .timeout(Duration::from_secs(30))
        .configure_client(|client| client.redirect(github_redirect_policy()))
        .version_comparator(move |current, remote| channel.accepts(&current, &remote.version))
        .build().map_err(|_| "update_unavailable")?;
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15))
        .redirect(github_redirect_policy()).build().map_err(|_| "update_check_failed")?;
    let indexes: &[UpdateChannel] = match channel {
        UpdateChannel::Stable => &[UpdateChannel::Stable],
        UpdateChannel::Preview => &[UpdateChannel::Stable, UpdateChannel::Preview],
    };
    let mut candidate: Option<Update> = None;
    let mut unpublished_channels = Vec::new();
    let mut channel_versions = Vec::new();
    for index in indexes {
        let Some(manifest) = fetch_manifest(&client, *index).await? else {
            unpublished_channels.push(*index);
            continue;
        };
        let manifest_version = manifest.get("version").and_then(|value| value.as_str())
            .and_then(|value| Version::parse(value.trim_start_matches('v')).ok())
            .ok_or("update_invalid_manifest")?;
        if *index == UpdateChannel::Stable && !manifest_version.pre.is_empty() {
            return Err("update_invalid_stable_manifest".into());
        }
        // The official updater retains version, platform, signature and installation checks.
        let update = updater.restore_update(manifest).map_err(|_| "update_invalid_manifest")?;
        channel_versions.push(manifest_version);
        if let Some(mut update) = update {
            update.timeout = Some(Duration::from_secs(600));
            validate_download(&update)?;
            if candidate.as_ref().is_none_or(|previous| {
                Version::parse(&update.version).unwrap().cmp_precedence(
                    &Version::parse(&previous.version).unwrap()).is_gt()
            }) {
                candidate = Some(update);
            }
        }
    }
    let (status, channel_version) = summarize_channel_versions(&app.package_info().version, &channel_versions);
    let available = if let Some(update) = candidate {
        validate_download(&update)?;
        let id = uuid::Uuid::new_v4().to_string();
        let result = AvailableUpdate {
            id: id.clone(),
            release_url: format!("{RELEASES_URL}/tag/v{}", update.version),
            version: update.version.clone(),
            notes: update.body.clone(),
            artifact: None,
        };
        *pending = Some(PendingUpdate { id, update });
        Some(result)
    } else { None };
    Ok(UpdateCheck { current_version: app.package_info().version.to_string(), channel, channel_version, status,
        support: installation_support(), available, unpublished_channels })
}

fn github_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 { return attempt.error("update_redirect_limit"); }
        let url = attempt.url();
        if url.scheme() == "https" && matches!(url.host_str(), Some("github.com"
            | "release-assets.githubusercontent.com" | "objects.githubusercontent.com"
            | "github-releases.githubusercontent.com")) {
            attempt.follow()
        } else { attempt.error("update_untrusted_redirect") }
    })
}

async fn fetch_manifest(client: &reqwest::Client, channel: UpdateChannel)
    -> Result<Option<serde_json::Value>, String>
{
    let mut response = client.get(channel.endpoint()).header("Accept", "application/json")
        .send().await.map_err(|_| "update_check_failed")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
    if !response.status().is_success() { return Err("update_server_failed".into()); }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "update_check_failed")? {
        if bytes.len().saturating_add(chunk.len()) > 256 * 1024 {
            return Err("update_invalid_manifest".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|_| "update_invalid_manifest".into())
}

fn validate_download(update: &Update) -> Result<(), String> {
    let version = Version::parse(&update.version).map_err(|_| "update_invalid_version")?;
    let url = &update.download_url;
    let prefix = format!("/qiyuanhuakai/dezirclip/releases/download/v{version}/");
    let asset = url.path().strip_prefix(&prefix).unwrap_or_default();
    let expected = expected_asset_name(std::env::consts::OS, std::env::consts::ARCH, &version)
        .ok_or("update_unsupported_platform")?;
    let decoded_asset = urlencoding::decode(asset).map_err(|_| "update_untrusted_download")?;
    if url.scheme() != "https" || url.host_str() != Some("github.com")
        || url.port().is_some() || !url.username().is_empty() || url.password().is_some()
        || url.query().is_some() || url.fragment().is_some()
        || decoded_asset != expected
    {
        return Err("update_untrusted_download".into());
    }
    validate_signature_metadata(&version, &expected, &update.signature)
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    update_id: String,
    on_progress: Channel<UpdateProgress>,
) -> Result<(), String> {
    let mut pending = state.pending.try_lock().map_err(|_| "update_busy")?;
    if !installation_support().can_install {
        return Err("update_installation_unsupported".into());
    }
    let selected = pending.as_ref().filter(|value| value.id == update_id)
        .ok_or("update_check_required")?;
    validate_download(&selected.update)?;
    let mut downloaded = 0_u64;
    let bytes = selected.update.download(|chunk, total| {
        downloaded = downloaded.saturating_add(chunk as u64);
        let _ = on_progress.send(UpdateProgress::Downloading { downloaded, total });
    }, || {}).await.map_err(|_| "update_download_or_signature_failed")?;
    let _ = on_progress.send(UpdateProgress::Verified);
    if !installation_support().can_install {
        return Err("update_installation_unsupported".into());
    }
    let update = selected.update.clone();
    let host = state.host.clone();
    tauri::async_runtime::spawn_blocking(move || install_after_host_ready(host.as_ref(), || {
        let _ = on_progress.send(UpdateProgress::Installing);
        update.install(bytes).map_err(|_| "update_install_failed".to_string())
    }))
        .await.map_err(|_| "update_install_failed")?
        ?;
    *pending = None;
    crate::app::app_exit::request_app_exit();
    app.restart();
}
