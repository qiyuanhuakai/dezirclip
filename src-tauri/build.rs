#[cfg(not(feature = "desktop"))]
fn main() {}

#[cfg(feature = "desktop")]
fn main() {
    let toolkit_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../frontends/dzc-surface/internal/buildinfo/mygo-toolkit.json");
    println!("cargo:rerun-if-changed={}", toolkit_path.display());
    let toolkit: serde_json::Value =
        serde_json::from_slice(&std::fs::read(toolkit_path).expect("read MyGo toolkit manifest"))
            .expect("parse MyGo toolkit manifest");
    for (variable, key) in [
        ("DZC_MYGO_TOOLKIT_PATCH", "id"),
        ("DZC_MYGO_TOOLKIT_VERSION", "version"),
        ("DZC_MYGO_TOOLKIT_SOURCE", "source_revision"),
    ] {
        let value = toolkit[key]
            .as_str()
            .filter(|value| {
                !value.is_empty()
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
            })
            .unwrap_or_else(|| panic!("invalid MyGo toolkit manifest value: {key}"));
        println!("cargo:rustc-env={variable}={value}");
    }

    tauri_build::build()
}
