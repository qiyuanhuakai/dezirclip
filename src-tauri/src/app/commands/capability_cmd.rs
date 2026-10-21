/// Runtime evidence only: no clipboard body, portal grant or external network.
#[tauri::command]
pub async fn get_platform_capabilities(
) -> Result<dzc_platform::runtime::SessionCapabilityReport, String> {
    tauri::async_runtime::spawn_blocking(dzc_platform::runtime::session_capabilities)
        .await
        .map_err(|_| "PlatformCapabilityProbeFailed".to_string())
}

pub(crate) fn require_desktop_interaction() -> Result<(), String> {
    if dzc_platform::runtime::is_native_wayland() {
        Err("wayland_interaction_unavailable".to_string())
    } else {
        Ok(())
    }
}
