use base64::Engine;
use semver::Version;
use serde::{Deserialize, Serialize};

pub const RELEASES_URL: &str = "https://github.com/qiyuanhuakai/dezirclip/releases";

pub fn expected_asset_name(os: &str, arch: &str, version: &Version) -> Option<String> {
    let suffix = match (os, arch) {
        ("windows", "x86_64") => "x64-setup.exe",
        ("windows", "aarch64") => "arm64-setup.exe",
        ("linux", "x86_64") => "amd64.AppImage",
        ("linux", "aarch64") => "aarch64.AppImage",
        _ => return None,
    };
    Some(format!("DezirClip_{version}_{suffix}"))
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Stable,
    Preview,
}

impl UpdateChannel {
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Stable => "https://github.com/qiyuanhuakai/dezirclip/releases/download/updater-channels/stable.json",
            Self::Preview => "https://github.com/qiyuanhuakai/dezirclip/releases/download/updater-channels/preview.json",
        }
    }

    pub fn accepts(self, current: &Version, candidate: &Version) -> bool {
        candidate.cmp_precedence(current).is_gt()
            && (self == Self::Preview || candidate.pre.is_empty())
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckStatus {
    Available,
    Current,
    Ahead,
    Unpublished,
}

pub fn summarize_channel_versions(current: &Version, versions: &[Version]) -> (UpdateCheckStatus, Option<String>) {
    let Some(latest) = versions.iter().max_by(|left, right| left.cmp_precedence(right)) else {
        return (UpdateCheckStatus::Unpublished, None);
    };
    let status = match latest.cmp_precedence(current) {
        std::cmp::Ordering::Greater => UpdateCheckStatus::Available,
        std::cmp::Ordering::Equal => UpdateCheckStatus::Current,
        std::cmp::Ordering::Less => UpdateCheckStatus::Ahead,
    };
    (status, Some(latest.to_string()))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateInstallation {
    Nsis,
    AppImage,
    AppImageReadOnly,
    PackageManager,
    Portable,
    Unpackaged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSupport {
    pub installation: UpdateInstallation,
    pub can_install: bool,
    pub reason: &'static str,
    pub releases_url: &'static str,
}

impl UpdateSupport {
    pub fn new(installation: UpdateInstallation) -> Self {
        let (can_install, reason) = match installation {
            UpdateInstallation::Nsis => (true, "nsis_in_place"),
            UpdateInstallation::AppImage => (true, "appimage_in_place"),
            UpdateInstallation::AppImageReadOnly => (false, "appimage_not_writable"),
            UpdateInstallation::PackageManager => (false, "use_package_manager"),
            UpdateInstallation::Portable => (false, "replace_portable_manually"),
            UpdateInstallation::Unpackaged => (false, "install_supported_bundle"),
        };
        Self { installation, can_install, reason, releases_url: RELEASES_URL }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub id: String,
    pub version: String,
    pub notes: Option<String>,
    pub release_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current_version: String,
    pub channel: UpdateChannel,
    pub channel_version: Option<String>,
    pub status: UpdateCheckStatus,
    pub support: UpdateSupport,
    pub available: Option<AvailableUpdate>,
    pub unpublished_channels: Vec<UpdateChannel>,
}

pub fn validate_signature_metadata(version: &Version, expected: &str, encoded: &str) -> Result<(), String> {
    let signature = base64::engine::general_purpose::STANDARD.decode(encoded.trim())
        .map_err(|_| "update_invalid_signature")?;
    let signature = std::str::from_utf8(&signature).map_err(|_| "update_invalid_signature")?;
    let signed_version = signature.lines().find_map(|line| line.strip_prefix("trusted comment: "))
        .and_then(|comment| comment.split('\t').find_map(|field| field.strip_prefix("version:")))
        .and_then(|value| Version::parse(value.trim_start_matches('v')).ok());
    if signed_version.as_ref() != Some(version) {
        return Err("update_signature_version_mismatch".into());
    }
    let signed_file = signature.lines().find_map(|line| line.strip_prefix("trusted comment: "))
        .and_then(|comment| comment.split('\t').find_map(|field| field.strip_prefix("file:")));
    if signed_file != Some(expected) { return Err("update_invalid_signature".into()); }
    Ok(())
}

#[derive(Deserialize)]
pub struct ManualIndexes {
    channel: UpdateChannel,
    indexes: Vec<Option<ManualManifest>>,
}

#[derive(Deserialize)]
struct ManualManifest {
    version: String,
    notes: Option<String>,
    platforms: std::collections::HashMap<String, ManualAsset>,
}

#[derive(Deserialize)]
struct ManualAsset { url: String, signature: String }

/// Evaluate the two fixed channel indexes for a manual desktop distribution.
/// The desktop downloads metadata. This policy never opens URLs or installs an artifact.
pub fn check_manual_indexes(input: ManualIndexes, current: &Version, os: &str, arch: &str) -> Result<UpdateCheck, String> {
    let channels: &[UpdateChannel] = match input.channel {
        UpdateChannel::Stable => &[UpdateChannel::Stable],
        UpdateChannel::Preview => &[UpdateChannel::Stable, UpdateChannel::Preview],
    };
    if input.indexes.len() != channels.len() { return Err("update_invalid_manifest".into()); }
    let platform = format!("{os}-{arch}");
    let mut versions = Vec::new();
    let mut unpublished_channels = Vec::new();
    let mut candidate = None;
    for (&channel, manifest) in channels.iter().zip(input.indexes) {
        let Some(manifest) = manifest else { unpublished_channels.push(channel); continue };
        let version = Version::parse(manifest.version.strip_prefix('v').unwrap_or(&manifest.version))
            .map_err(|_| "update_invalid_manifest")?;
        if channel == UpdateChannel::Stable && !version.pre.is_empty() { return Err("update_invalid_stable_manifest".into()); }
        if channel.accepts(current, &version) {
            let asset = manifest.platforms.get(&platform).ok_or("update_invalid_manifest")?;
            let expected = expected_asset_name(os, arch, &version).ok_or("update_unsupported_platform")?;
            let prefix = format!("{RELEASES_URL}/download/v{version}/");
            let file = asset.url.strip_prefix(&prefix).ok_or("update_untrusted_download")?;
            if urlencoding::decode(file).map_err(|_| "update_untrusted_download")? != expected {
                return Err("update_untrusted_download".into());
            }
            validate_signature_metadata(&version, &expected, &asset.signature)?;
            if candidate.as_ref().is_none_or(|(previous, _)| version.cmp_precedence(previous).is_gt()) {
                candidate = Some((version.clone(), AvailableUpdate { id: version.to_string(), version: version.to_string(),
                    notes: manifest.notes, release_url: format!("{RELEASES_URL}/tag/v{version}") }));
            }
        }
        versions.push(version);
    }
    let (status, channel_version) = summarize_channel_versions(current, &versions);
    Ok(UpdateCheck { current_version: current.to_string(), channel: input.channel, channel_version, status,
        support: UpdateSupport::new(UpdateInstallation::Portable), available: candidate.map(|(_, result)| result), unpublished_channels })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_zip_checks_both_channels_and_preserves_signed_asset_identity() {
        let current = Version::parse("1.0.0").unwrap();
        let evaluate = |indexes| {
            let input = serde_json::from_value(serde_json::json!({"channel":"preview","indexes":indexes})).unwrap();
            check_manual_indexes(input, &current, "windows", "x86_64")
        };
        let manifest = |version: &str| {
            let file = format!("DezirClip_{version}_x64-setup.exe");
            let signature = base64::engine::general_purpose::STANDARD.encode(format!("trusted comment: file:{file}\tversion:{version}"));
            serde_json::json!({"version":version,"notes":"Changes","platforms":{"windows-x86_64":{
                "url":format!("{RELEASES_URL}/download/v{version}/{file}"),"signature":signature}}})
        };
        let stable = manifest("2.0.0");
        let preview = manifest("1.9.0-alpha.10");
        let result = evaluate(serde_json::json!([stable,preview])).unwrap();
        assert_eq!(result.available.unwrap().release_url, format!("{RELEASES_URL}/tag/v2.0.0"));
        assert!(!result.support.can_install);
        let result = evaluate(serde_json::json!([null,null])).unwrap();
        assert_eq!(result.status, UpdateCheckStatus::Unpublished);
        assert_eq!(result.unpublished_channels.len(), 2);
        assert_eq!(evaluate(serde_json::json!([preview,null])).err().unwrap(),"update_invalid_stable_manifest");
        let mut wrong = stable.clone();
        wrong["platforms"]["windows-x86_64"]["signature"] = preview["platforms"]["windows-x86_64"]["signature"].clone();
        assert_eq!(evaluate(serde_json::json!([wrong,null])).err().unwrap(),"update_signature_version_mismatch");
        wrong["platforms"]["windows-x86_64"]["url"] = "https://example.invalid/file".into();
        assert_eq!(evaluate(serde_json::json!([wrong,null])).err().unwrap(),"update_untrusted_download");
    }

    #[test]
    fn channels_follow_semver_precedence_without_downgrades() {
        let version = |value: &str| Version::parse(value).unwrap();
        let cases = [
            ("0.1.20", "0.2.0-alpha.1", false, true),
            ("0.2.0-alpha.2", "0.2.0-alpha.10", false, true),
            ("0.2.0-rc.1", "0.2.0", true, true),
            ("0.2.0", "0.2.0-rc.9", false, false),
            ("0.2.0-alpha.2", "0.1.21", false, false),
            ("0.2.0", "0.2.0+new-build", false, false),
            ("0.2.0+old-build", "0.2.0+new-build", false, false),
            ("0.2.0", "0.2.1", true, true),
        ];
        for (current, candidate, stable, preview) in cases {
            assert_eq!(UpdateChannel::Stable.accepts(&version(current), &version(candidate)), stable);
            assert_eq!(UpdateChannel::Preview.accepts(&version(current), &version(candidate)), preview);
        }
    }

    #[test]
    fn update_results_distinguish_equal_ahead_newer_and_unpublished_channels() {
        let current = Version::parse("0.2.0-alpha.3").unwrap();
        assert_eq!(summarize_channel_versions(&current, &[]), (UpdateCheckStatus::Unpublished, None));
        for (offered, expected) in [
            ("0.2.0-alpha.2", UpdateCheckStatus::Ahead),
            ("0.2.0-alpha.3", UpdateCheckStatus::Current),
            ("0.2.0-alpha.4", UpdateCheckStatus::Available),
            ("0.2.0-alpha.10", UpdateCheckStatus::Available),
        ] {
            assert_eq!(summarize_channel_versions(&current, &[Version::parse(offered).unwrap()]),
                (expected, Some(offered.to_string())));
        }
    }

    #[test]
    fn missing_auxiliary_index_does_not_hide_a_published_channel_version() {
        let current = Version::parse("0.2.0-alpha.3").unwrap();
        let preview = Version::parse("0.2.0-alpha.2").unwrap();
        assert_eq!(summarize_channel_versions(&current, &[preview.clone()]),
            (UpdateCheckStatus::Ahead, Some(preview.to_string())));
        assert_eq!(summarize_channel_versions(&current, &[preview, Version::parse("0.2.0").unwrap()]),
            (UpdateCheckStatus::Available, Some("0.2.0".to_string())));
    }

    #[test]
    fn update_result_ignores_build_metadata_and_serializes_explicit_states() {
        let current = Version::parse("0.2.0+installed").unwrap();
        let offered = Version::parse("0.2.0+repacked").unwrap();
        assert_eq!(summarize_channel_versions(&current, &[offered]),
            (UpdateCheckStatus::Current, Some("0.2.0+repacked".to_string())));
        assert_eq!(serde_json::to_string(&UpdateCheckStatus::Unpublished).unwrap(), "\"unpublished\"");
    }

    #[test]
    fn only_supported_installations_can_be_replaced() {
        for installation in [UpdateInstallation::AppImageReadOnly, UpdateInstallation::PackageManager,
            UpdateInstallation::Portable, UpdateInstallation::Unpackaged] {
            assert!(!UpdateSupport::new(installation).can_install);
        }
        assert!(UpdateSupport::new(UpdateInstallation::Nsis).can_install);
        assert!(UpdateSupport::new(UpdateInstallation::AppImage).can_install);
    }

    #[test]
    fn only_published_platforms_have_exact_artifact_names() {
        let version = Version::parse("0.2.0-alpha.1").unwrap();
        assert_eq!(expected_asset_name("windows", "x86_64", &version).as_deref(),
            Some("DezirClip_0.2.0-alpha.1_x64-setup.exe"));
        assert_eq!(expected_asset_name("linux", "aarch64", &version).as_deref(),
            Some("DezirClip_0.2.0-alpha.1_aarch64.AppImage"));
        assert!(expected_asset_name("linux", "x86", &version).is_none());
    }
}
