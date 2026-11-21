use std::{
    io::{self, Read, Write},
    process::{ChildStdin, Command, Stdio},
    sync::{atomic::{AtomicBool, Ordering}, Mutex},
};

static EXPLICIT_APP_EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static RELAUNCH_PIPE: Mutex<Option<ChildStdin>> = Mutex::new(None);
const RELAUNCH_ARG: &str = "--dezirclip-relaunch";

pub fn request_app_exit() {
    EXPLICIT_APP_EXIT_REQUESTED.store(true, Ordering::SeqCst);
}

pub fn relaunch_after_shutdown(app: &tauri::AppHandle, shutdown: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    use tauri::Manager;
    let mut waiting = RELAUNCH_PIPE.lock().map_err(|_| io::Error::other("Relaunch state unavailable"))?;
    if waiting.is_none() {
        let executable = tauri::process::current_binary(&app.env())?;
        let mut child = Command::new(executable).arg(RELAUNCH_ARG).stdin(Stdio::piped()).spawn()?;
        let mut pipe = child.stdin.take().ok_or_else(|| io::Error::other("Missing relaunch pipe"))?;
        shutdown()?;
        pipe.write_all(b"R")?;
        // Keep the pipe open until process exit, after the single-instance lock is released.
        *waiting = Some(pipe);
        request_app_exit();
    }
    drop(waiting);
    app.exit(0);
    Ok(())
}

pub fn relaunch_prepared() -> bool {
    RELAUNCH_PIPE.lock().is_ok_and(|pipe| pipe.is_some())
}

fn wait_for_relaunch(mut pipe: impl Read) -> bool {
    let mut byte = [0];
    if pipe.read_exact(&mut byte).is_err() || byte != *b"R" {
        return false;
    }
    pipe.read_exact(&mut byte).is_err_and(|error| error.kind() == io::ErrorKind::UnexpectedEof)
}

pub fn accept_relaunch() -> bool {
    std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(RELAUNCH_ARG))
        || wait_for_relaunch(io::stdin().lock())
}

pub fn should_prevent_current_exit_requested() -> bool {
    should_prevent_exit_requested(EXPLICIT_APP_EXIT_REQUESTED.load(Ordering::SeqCst))
}

pub fn should_prevent_exit_requested(explicit_exit_requested: bool) -> bool {
    !explicit_exit_requested
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevents_runtime_exit_when_no_explicit_exit_was_requested() {
        assert!(should_prevent_exit_requested(false));
    }

    #[test]
    fn allows_runtime_exit_when_user_requested_exit() {
        assert!(!should_prevent_exit_requested(true));
    }

    #[test]
    fn replacement_requires_approval_followed_by_pipe_close() {
        assert!(wait_for_relaunch(&b"R"[..]));
        assert!(!wait_for_relaunch(&b""[..]));
        assert!(!wait_for_relaunch(&b"X"[..]));
        assert!(!wait_for_relaunch(&b"RR"[..]));
    }
}
