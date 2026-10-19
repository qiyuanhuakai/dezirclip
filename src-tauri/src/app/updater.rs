use crate::domain::update::{expected_asset_name, UpdateChannel, UpdateSupport, RELEASES_URL};
use crate::infrastructure::update_support::installation_support;
use base64::Engine;
use semver::Version;
use serde::Serialize;
use std::time::Duration;
use tauri::{ipc::Channel, AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct UpdaterState(Mutex<Option<PendingUpdate>>);

struct PendingUpdate {
    id: String,
    update: Update,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    id: String,
    version: String,
    notes: Option<String>,
    release_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    current_version: String,
    channel: UpdateChannel,
    support: UpdateSupport,
    available: Option<AvailableUpdate>,
    unpublished_channels: Vec<UpdateChannel>,
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
    let mut pending = state.0.try_lock().map_err(|_| "update_busy")?;
    *pending = None;
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
    if unpublished_channels.len() == indexes.len() {
        return Err("update_channel_not_published".into());
    }
    let available = if let Some(update) = candidate {
        validate_download(&update)?;
        let id = uuid::Uuid::new_v4().to_string();
        let result = AvailableUpdate {
            id: id.clone(),
            release_url: format!("{RELEASES_URL}/tag/v{}", update.version),
            version: update.version.clone(),
            notes: update.body.clone(),
        };
        *pending = Some(PendingUpdate { id, update });
        Some(result)
    } else { None };
    Ok(UpdateCheck { current_version: app.package_info().version.to_string(), channel,
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
    let signature = base64::engine::general_purpose::STANDARD.decode(update.signature.trim())
        .map_err(|_| "update_invalid_signature")?;
    let signature = std::str::from_utf8(&signature).map_err(|_| "update_invalid_signature")?;
    let signed_version = signature.lines().find_map(|line| line.strip_prefix("trusted comment: "))
        .and_then(|comment| comment.split('\t').find_map(|field| field.strip_prefix("version:")))
        .and_then(|value| Version::parse(value.trim_start_matches('v')).ok());
    if signed_version.as_ref() != Some(&version) {
        return Err("update_signature_version_mismatch".into());
    }
    let signed_file = signature.lines().find_map(|line| line.strip_prefix("trusted comment: "))
        .and_then(|comment| comment.split('\t').find_map(|field| field.strip_prefix("file:")));
    if signed_file != Some(expected.as_str()) { return Err("update_invalid_signature".into()); }
    Ok(())
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, UpdaterState>,
    update_id: String,
    on_progress: Channel<UpdateProgress>,
) -> Result<(), String> {
    let mut pending = state.0.try_lock().map_err(|_| "update_busy")?;
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
    let _ = on_progress.send(UpdateProgress::Installing);
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await.map_err(|_| "update_install_failed")?
        .map_err(|_| "update_install_failed")?;
    *pending = None;
    crate::app::app_exit::request_app_exit();
    app.restart();
}
