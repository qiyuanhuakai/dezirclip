fn main() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(std::env::var("RUNNER_ENVIRONMENT").as_deref(), Ok("github-hosted"));
    let directory = std::path::PathBuf::from(std::env::args_os().nth(1).expect("isolated diagnostic directory"));
    dzc_platform::crash_diagnostics::install(&directory);
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::System::Diagnostics::Debug::{SetErrorMode, RaiseException, SEM_NOGPFAULTERRORBOX, SEM_FAILCRITICALERRORS};
        SetErrorMode(SEM_NOGPFAULTERRORBOX | SEM_FAILCRITICALERRORS);
        RaiseException(0xc0000005, 0, None);
    }
}
