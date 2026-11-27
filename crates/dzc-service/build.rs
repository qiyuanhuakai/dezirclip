fn main() {
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        tauri_winres::WindowsResource::new()
            .set_icon(icon.to_str().expect("icon path must be UTF-8"))
            .set("ProductName", "DezirClip")
            .set("FileDescription", "DezirClip")
            .compile_for(&["dzc-service"])
            .expect("embed the Service icon");
    }
}
