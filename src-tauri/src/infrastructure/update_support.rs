use crate::domain::update::{UpdateInstallation, UpdateSupport};
use std::path::Path;
use tauri::utils::{config::BundleType, platform::bundle_type};

pub fn installation_support() -> UpdateSupport {
    if cfg!(feature = "portable") {
        return UpdateSupport::new(UpdateInstallation::Portable);
    }
    if std::env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").exists() {
        return UpdateSupport::new(UpdateInstallation::PackageManager);
    }
    let installation = match bundle_type() {
        Some(BundleType::Nsis) => UpdateInstallation::Nsis,
        Some(BundleType::AppImage) => {
            let writable = std::env::var_os("APPIMAGE")
                .map(std::path::PathBuf::from)
                .is_some_and(|path| appimage_is_writable(&path));
            if writable { UpdateInstallation::AppImage } else { UpdateInstallation::AppImageReadOnly }
        }
        Some(BundleType::Deb | BundleType::Rpm) => UpdateInstallation::PackageManager,
        _ => UpdateInstallation::Unpackaged,
    };
    UpdateSupport::new(installation)
}

fn appimage_is_writable(path: &Path) -> bool {
    let Some(parent) = path.parent() else { return false; };
    if !path.is_file() || std::fs::OpenOptions::new().write(true).open(path).is_err() {
        return false;
    }
    let probe = parent.join(format!(".dezirclip-update-{}", uuid::Uuid::new_v4()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            drop(file);
            std::fs::remove_file(probe).is_ok()
        }
        Err(_) => false,
    }
}
