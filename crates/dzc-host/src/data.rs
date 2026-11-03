//! One data-path resolver for optional Host helpers and linked clients.
use std::{io, path::{Path, PathBuf}};
pub(crate) fn installed_config_directory() -> Result<PathBuf, String> {
    Ok(dirs::data_dir().ok_or("cannot locate the application data directory")?.join("io.github.qiyuanhuakai.dezirclip"))
}
pub fn database_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("DEZIRCLIP_DB_PATH").or_else(|| std::env::var_os("TIEZ_DB_PATH")) {
        if path.is_empty() { return Err("database path override is empty".into()); }
        return Ok(path.into());
    }
    let current = installed_config_directory()?;
    let executable = std::env::current_exe().ok();
    Ok(dzc_core::api::resolve_database_path(&current, &current.with_file_name("com.tiez.clipboard"), executable.as_deref().and_then(std::path::Path::parent)))
}
pub fn canonical_directory(database: &std::path::Path) -> Result<PathBuf, String> {
    if database.try_exists().map_err(|_| "HostDataDirectoryUnavailable")? {
        let actual = database.canonicalize().map_err(|_| "HostDataDirectoryUnavailable")?;
        return actual.parent().map(std::path::Path::to_owned).ok_or_else(|| "HostDataDirectoryUnavailable".into());
    }
    let directory = database.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(directory).map_err(|_| "HostDataDirectoryUnavailable")?;
    directory.canonicalize().map_err(|_| "HostDataDirectoryUnavailable".into())
}

pub(crate) fn installed_move_directories(database: &Path, destination: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let executable = std::env::current_exe()?;
    if std::env::var_os("DEZIRCLIP_DB_PATH").is_some() || std::env::var_os("TIEZ_DB_PATH").is_some() {
        return Err(io::Error::other("A database path override is active; change the override to relocate the profile"));
    }
    if executable.with_file_name("data").is_dir() {
        return Err(io::Error::other("Move the complete portable directory to relocate its data"));
    }
    let source = database.parent().ok_or_else(|| io::Error::other("Missing profile directory"))?;
    database.canonicalize()?;
    let source_dir = source.canonicalize()?;
    let target_dir = destination.canonicalize()?;
    if !target_dir.is_dir() { return Err(io::Error::other("Destination is not a directory")); }

    Ok((source_dir, target_dir))
}

pub(crate) fn reserve_profile(directory: &Path) -> io::Result<dzc_platform::ipc::Listener> {
    dzc_platform::ipc::Endpoint::for_current_session(directory)?.bind()
        .map_err(|e| io::Error::new(e.kind(), format!("Could not reserve the profile; close its desktop Host or Service: {e}")))
}

pub(crate) fn move_redirect(database: &Path) -> io::Result<PathBuf> {
    if database_path().map_err(io::Error::other)?.canonicalize()? != database.canonicalize()? {
        return Err(io::Error::other("The active profile changed before migration"));
    }
    Ok(installed_config_directory().map_err(io::Error::other)?.join("datapath.txt"))
}
