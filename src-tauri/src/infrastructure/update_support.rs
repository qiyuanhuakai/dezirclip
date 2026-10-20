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
    if !path.is_file() || !appimage_file_is_writable(path) {
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

#[cfg(target_os = "linux")]
fn appimage_file_is_writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false; };
    // An executing AppImage can reject writable opens with ETXTBSY. Installation replaces
    // its directory entry, so check effective-user access without opening the executable.
    unsafe { libc::faccessat(libc::AT_FDCWD, path.as_ptr(), libc::W_OK, libc::AT_EACCESS) == 0 }
}

#[cfg(not(target_os = "linux"))]
fn appimage_file_is_writable(path: &Path) -> bool {
    std::fs::OpenOptions::new().write(true).open(path).is_ok()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn appimage_support_requires_writable_file_and_parent() {
        let directory = std::env::temp_dir().join(format!("dzc-appimage-support-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let image = directory.join("fixture.AppImage");
        std::fs::write(&image, b"fixture").unwrap();
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(appimage_is_writable(&image));
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(!appimage_is_writable(&image));
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(!appimage_is_writable(&image));
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(appimage_is_writable(&image));
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_file(&image).unwrap();
        assert!(!appimage_is_writable(&image));
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn executing_writable_appimage_is_not_misclassified_as_read_only() {
        let directory = std::env::temp_dir().join(format!("dzc-running-appimage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let image = directory.join("fixture.AppImage");
        std::fs::copy("/bin/sleep", &image).unwrap();
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut process = std::process::Command::new(&image).arg("30").spawn().unwrap();
        let busy = std::fs::OpenOptions::new().write(true).open(&image).unwrap_err();
        assert_eq!(busy.raw_os_error(), Some(libc::ETXTBSY));
        let writable = appimage_is_writable(&image);
        process.kill().unwrap();
        process.wait().unwrap();
        std::fs::remove_file(&image).unwrap();
        std::fs::remove_dir(directory).unwrap();
        assert!(writable);
    }
}
