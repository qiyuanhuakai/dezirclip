fn main() {
    ensure_sidecar_placeholder();
    tauri_build::build()
}

fn ensure_sidecar_placeholder() {
    let target = match std::env::var("TARGET") {
        Ok(target) => target,
        Err(_) => return,
    };
    let extension = if target.contains("windows") { ".exe" } else { "" };
    let path = std::path::PathBuf::from("binaries").join(format!("dzc-{target}{extension}"));
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, []);
}
