#[cfg(target_os = "windows")]
pub fn release_idle_working_set() {
    crate::infrastructure::windows_api::process_memory::release_idle_working_set();
}

#[cfg(not(target_os = "windows"))]
pub fn release_idle_working_set() {}
