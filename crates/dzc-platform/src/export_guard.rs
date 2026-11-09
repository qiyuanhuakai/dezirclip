//! Independent cleanup for external editors which do not share deletion.
use std::{io::{BufRead, Read, Write}, path::{Path, PathBuf}, process::{Child, Command, Stdio}, time::Duration};
use std::os::windows::process::CommandExt;

pub(crate) struct Guardian(Option<Child>);

impl Guardian {
    pub(crate) fn start(directory: &Path, paths: &[String]) -> Result<Self, String> {
        let mut command = Command::new(std::env::current_exe().map_err(|_| "PrivateExportGuardianUnavailable")?);
        #[cfg(not(test))]
        command.arg("--dzc-export-guardian");
        #[cfg(test)]
        command.args(["--exact", "export_guard::tests::guardian_subprocess", "--nocapture"])
            .env("DZC_GUARDIAN_TEST_CHILD", "1");
        let child = command.creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().map_err(|_| "PrivateExportGuardianUnavailable")?;
        let mut guard = Self(Some(child));
        let child = guard.0.as_mut().unwrap();
        let payload = serde_json::to_vec(&(directory, paths)).map_err(|_| "PrivateExportGuardianUnavailable")?;
        let input = child.stdin.as_mut().ok_or("PrivateExportGuardianUnavailable")?;
        input.write_all(&(payload.len() as u32).to_le_bytes()).and_then(|_| input.write_all(&payload)).and_then(|_| input.flush())
            .map_err(|_| "PrivateExportGuardianUnavailable")?;
        let output = child.stdout.take().ok_or("PrivateExportGuardianUnavailable")?;
        let (ready, wait) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new().name("dzc-export-ready".into()).spawn(move || {
            let mut reader = std::io::BufReader::new(output).take(4096);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) != 0 {
                if line.trim_end().ends_with("DZC_EXPORT_GUARD_READY") {
                    let _ = ready.send(());
                    break;
                }
                line.clear();
            }
        }).map_err(|_| "PrivateExportGuardianUnavailable")?;
        if wait.recv_timeout(Duration::from_secs(5)).is_err() {
            let _ = child.kill();
            return Err("PrivateExportGuardianUnavailable".into());
        }
        Ok(guard)
    }
}

impl Drop for Guardian {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            child.stdin.take();
            // Closing the pipe also happens on abrupt owner termination. The
            // guardian keeps retrying while a non-sharing editor holds a file.
            let _ = std::thread::Builder::new().name("dzc-export-reaper".into()).spawn(move || { let _ = child.wait(); });
        }
    }
}

fn run() -> Result<(), String> {
    let _ = crate::process_memory::background_service();
    let mut input = std::io::stdin().lock();
    let mut size = [0; 4];
    input.read_exact(&mut size).map_err(|_| "InvalidGuardianInput")?;
    let size = u32::from_le_bytes(size) as usize;
    if size == 0 || size > 65536 { return Err("InvalidGuardianInput".into()); }
    let mut payload = vec![0; size];
    input.read_exact(&mut payload).map_err(|_| "InvalidGuardianInput")?;
    let (directory, paths): (PathBuf, Vec<PathBuf>) = serde_json::from_slice(&payload).map_err(|_| "InvalidGuardianInput")?;
    let name = directory.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix("dzc-private-"))
        .ok_or("InvalidGuardianDirectory")?;
    if uuid::Uuid::parse_str(name).is_err() || directory.parent() != Some(std::env::temp_dir().as_path())
        || paths.is_empty() || paths.len() > 64 { return Err("InvalidGuardianDirectory".into()); }
    for path in &paths {
        if path.parent().and_then(Path::parent) != Some(directory.as_path()) { return Err("InvalidGuardianPath".into()); }
        for parent in [directory.as_path(), path.parent().unwrap()] {
            use std::os::windows::fs::MetadataExt;
            let metadata = std::fs::symlink_metadata(parent).map_err(|_| "InvalidGuardianPath")?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 { return Err("InvalidGuardianPath".into()); }
        }
    }
    println!("DZC_EXPORT_GUARD_READY");
    std::io::stdout().flush().map_err(|_| "GuardianUnavailable")?;
    let mut byte = [0];
    while input.read(&mut byte).unwrap_or(0) != 0 {}
    loop {
        let mut pending = false;
        for path in &paths {
            if let Err(error) = std::fs::remove_file(path) { pending |= error.kind() != std::io::ErrorKind::NotFound; }
        }
        if !pending { break; }
        std::thread::sleep(Duration::from_millis(100));
    }
    for path in &paths { if let Some(parent) = path.parent() { let _ = std::fs::remove_dir(parent); } }
    let _ = std::fs::remove_dir(directory);
    Ok(())
}

pub(crate) fn dispatch() {
    if std::env::args_os().skip(1).eq([std::ffi::OsString::from("--dzc-export-guardian")]) {
        std::process::exit(if run().is_ok() { 0 } else { 1 });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn guardian_subprocess() {
        if std::env::var_os("DZC_GUARDIAN_TEST_CHILD").is_some() { super::run().unwrap(); }
    }
}
