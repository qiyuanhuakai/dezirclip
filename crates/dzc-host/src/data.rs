//! One data-path resolver for optional Host helpers and linked clients.
use std::path::PathBuf;
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
