#[cfg(not(feature = "desktop"))]
fn main() {}

#[cfg(feature = "desktop")]
fn main() {
    // dzc is registered as a regular Cargo bundle binary. An explicit legacy
    // externalBin override can still copy a staged CLI over target/<profile>
    // while Cargo considers its artifact fresh. This build-script-only guard
    // prevents that copy; the bundle hook prepares the regular CLI input.
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
