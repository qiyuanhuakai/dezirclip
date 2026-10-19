use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows::Win32::System::Threading::GetCurrentProcess;

/// Release resident pages from this process after its webviews have gone away.
pub fn release_idle_working_set() {
    // The pseudo-handle belongs to the current process and must not be closed.
    if let Err(err) = unsafe { EmptyWorkingSet(GetCurrentProcess()) } {
        crate::warn!("[process-memory] Failed to release the idle working set: {err}");
    }
}
