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

#[cfg(test)]
mod tests {
    use super::*;

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
