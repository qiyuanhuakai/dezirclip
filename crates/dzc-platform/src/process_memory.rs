//! Memory replacement priority for processes which have no foreground UI.
pub fn background_service() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessInformation,
            ProcessMemoryPriority, MEMORY_PRIORITY_INFORMATION, MEMORY_PRIORITY_VERY_LOW};
        let priority = MEMORY_PRIORITY_INFORMATION { MemoryPriority: MEMORY_PRIORITY_VERY_LOW };
        // Only this process is affected; clipboard/OCR CPU scheduling is unchanged.
        unsafe {
            SetProcessInformation(GetCurrentProcess(), ProcessMemoryPriority,
                (&priority as *const MEMORY_PRIORITY_INFORMATION).cast(),
                std::mem::size_of_val(&priority) as u32)
        }.map_err(|error| error.to_string())?;
    }
    Ok(())
}
