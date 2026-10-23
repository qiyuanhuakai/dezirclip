#[cfg(target_os = "linux")]
mod linux {
use base64::Engine;
use keyring::credential::CredentialApi;
use keyring::secret_service::SsCredential;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

const MARKER: &str = "synthetic-fixture-only\n";
const QA_KEY: [u8; 32] = [0x71; 32];

fn private_path(path: &Path, root: Option<&Path>, socket: bool) -> Result<PathBuf, &'static str> {
    let original = std::fs::symlink_metadata(path).map_err(|_| "FixturePathMissing")?;
    if original.file_type().is_symlink() { return Err("FixtureSymlinkRejected"); }
    let canonical = std::fs::canonicalize(path).map_err(|_| "FixturePathInvalid")?;
    if root.is_some_and(|root| !canonical.starts_with(root) || canonical == root) {
        return Err("FixturePathOutsideRoot");
    }
    if original.uid() != unsafe { libc::geteuid() } || original.mode() & 0o077 != 0 {
        return Err("FixtureOwnerOrModeRejected");
    }
    if socket && !original.file_type().is_socket() { return Err("FixtureBusNotSocket"); }
    if !socket && !original.is_dir() { return Err("FixturePathNotDirectory"); }
    Ok(canonical)
}

fn run() -> Result<(), &'static str> {
    let mut arguments = std::env::args().skip(1);
    let operation = arguments.next().ok_or("OperationRequired")?;
    if operation != "seed" && operation != "check" { return Err("InvalidOperation"); }
    let root = arguments.next().ok_or("FixtureRootRequired")?;
    if arguments.next().is_some() { return Err("UnexpectedArgument"); }
    let root = private_path(Path::new(&root), None, false)?;
    if !root.to_string_lossy().contains("dezirclip-") { return Err("FixtureRootNameRejected"); }
    let marker = root.join(".dzc-private-ss-qa-v1");
    let metadata = std::fs::symlink_metadata(&marker).map_err(|_| "FixtureMarkerMissing")?;
    if !metadata.is_file() || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0
        || std::fs::read_to_string(marker).map_err(|_| "FixtureMarkerUnreadable")? != MARKER {
        return Err("FixtureMarkerRejected");
    }
    for name in ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"] {
        let value = std::env::var_os(name).ok_or("FixtureEnvironmentMissing")?;
        private_path(Path::new(&value), Some(&root), false)?;
    }
    if std::env::var_os("DBUS_SYSTEM_BUS_ADDRESS").is_some() { return Err("SystemBusOverrideRejected"); }
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| "FixtureBusMissing")?;
    let socket = address.strip_prefix("unix:path=").ok_or("FixtureBusAddressRejected")?;
    if socket.contains([',', ';', '%']) { return Err("FixtureBusAddressRejected"); }
    private_path(Path::new(socket), Some(&root), true)?;
    let credential = SsCredential::new_with_target(None, "dezirclip", "encryption-master-key")
        .map_err(|_| "SecretServiceConstructionFailed")?;
    let expected = base64::engine::general_purpose::STANDARD.encode(QA_KEY);
    if operation == "seed" {
        match credential.get_password() {
            Err(keyring::Error::NoEntry) => {},
            Ok(_) => return Err("ExistingCredentialRefused"),
            Err(_) => return Err("SecretServiceReadUnavailable"),
        }
        credential.set_password(&expected).map_err(|_| "SecretServiceWriteFailed")?;
    }
    let stored = credential.get_password().map_err(|_| "SecretServiceReadbackFailed")?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(&stored)
        .map_err(|_| "SecretServiceReadbackInvalid")?;
    if decoded.as_slice() != QA_KEY || stored != expected { return Err("SecretServiceReadbackMismatch"); }
    println!("{{\"status\":\"passed\",\"operation\":\"{}\",\"backend\":\"explicit_secret_service\",\"key_bytes\":32,\"private_environment_verified\":true}}", operation);
    Ok(())
}

pub fn main() {
    if let Err(reason) = run() {
        eprintln!("{{\"status\":\"failed\",\"reason\":\"{}\"}}", reason);
        std::process::exit(1);
    }
}

}
#[cfg(target_os = "linux")]
fn main() { linux::main(); }
#[cfg(not(target_os = "linux"))]
fn main() { std::process::exit(1); }
