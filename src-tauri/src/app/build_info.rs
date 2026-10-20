#[tauri::command]
pub fn get_build_info() -> dzc_core::protocol::build_info::BuildInfo {
    dzc_core::protocol::build_info::BuildInfo::current(
        dzc_core::protocol::build_info::Component::Desktop, dzc_core::api::DATABASE_SCHEMA_VERSION)
}
