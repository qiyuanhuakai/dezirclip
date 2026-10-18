fn main() {
    // Tauri copies externalBin into target/<profile>, where Cargo also writes
    // our real dzc binary. A check or GUI build can otherwise replace it with
    // an old sidecar while Cargo still considers the CLI artifact fresh.
    // This override is local to the build-script process. The Tauri bundler
    // still reads the original configuration and packages the prepared sidecar.
    let mut config: serde_json::Value = std::env::var("TAURI_CONFIG")
        .map(|value| serde_json::from_str(&value).expect("TAURI_CONFIG must be valid JSON"))
        .unwrap_or_else(|_| serde_json::json!({}));
    let bundle = config
        .as_object_mut()
        .expect("TAURI_CONFIG must be an object")
        .entry("bundle")
        .or_insert_with(|| serde_json::json!({}));
    if !bundle.is_null() {
        let bundle = bundle.as_object_mut().expect("bundle must be an object");
        bundle.remove("external-bin");
        bundle.insert("externalBin".to_string(), serde_json::json!([]));
    }
    std::env::set_var("TAURI_CONFIG", config.to_string());
    tauri_build::build()
}
