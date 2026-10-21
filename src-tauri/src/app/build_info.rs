#[tauri::command]
pub fn get_build_info() -> dzc_core::protocol::build_info::BuildInfo {
    dzc_host::build_info(
        dzc_core::protocol::build_info::Component::Desktop, dzc_core::api::DATABASE_SCHEMA_VERSION)
}
