//! Native process creation flags for explicitly requested local helper startup.
use std::{ffi::OsString, io, path::Path, process::{Command, Stdio}};
use std::io::Read;

/// An exit observation opened before requesting shutdown. It never terminates
/// the peer and cannot accidentally follow a subsequently reused process ID.
pub struct ProcessExit {
    pid: u32,
    #[cfg(target_os="windows")]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(target_os="linux")]
    pidfd: std::os::fd::OwnedFd,
}
#[cfg(target_os="windows")]
unsafe impl Send for ProcessExit {}
impl ProcessExit {
    pub fn ensure_supported() -> io::Result<()> {
        drop(Self::open(&super::current_identity()?)?);
        Ok(())
    }
    pub fn open(peer: &super::PeerIdentity) -> io::Result<Self> {
        let pid = peer.pid().ok_or_else(super::denied)?;
        Self::open_pid(pid)
    }
    fn open_pid(pid: u32) -> io::Result<Self> {
        #[cfg(target_os="windows")]
        {
            use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid).map_err(io::Error::other)? };
            Ok(Self { pid, handle })
        }
        #[cfg(target_os="linux")]
        {
            use std::os::fd::FromRawFd;
            let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            if descriptor < 0 {
                let error = io::Error::last_os_error();
                return Err(if matches!(error.raw_os_error(),Some(libc::ENOSYS | libc::EPERM | libc::EACCES)) {
                    io::Error::new(io::ErrorKind::Unsupported,"ServiceProcessObservationUnsupported")
                } else { error });
            }
            Ok(Self { pid, pidfd: unsafe { std::os::fd::OwnedFd::from_raw_fd(descriptor as i32) } })
        }
    }
    /// Compare OS-reported executable identities while both process generations
    /// remain alive. No client-provided path or window property is trusted.
    pub fn shares_executable(&self, pid: u32) -> io::Result<bool> {
        let other = Self::open_pid(pid)?;
        if !self.is_running()? || !other.is_running()? { return Err(super::denied()); }
        #[cfg(target_os="windows")]
        let same = process_image(self.pid)?.eq_ignore_ascii_case(&process_image(pid)?);
        #[cfg(target_os="linux")]
        let same = {
            use std::os::unix::fs::MetadataExt;
            let left = std::fs::metadata(format!("/proc/{}/exe", self.pid))?;
            let right = std::fs::metadata(format!("/proc/{pid}/exe"))?;
            left.dev() == right.dev() && left.ino() == right.ino()
        };
        if !self.is_running()? || !other.is_running()? { return Err(super::denied()); }
        Ok(same)
    }
    pub fn wait(self, timeout: std::time::Duration) -> io::Result<()> { self.observe(timeout) }

    pub fn is_running(&self) -> io::Result<bool> {
        match self.observe(std::time::Duration::ZERO) {
            Ok(()) => Ok(false),
            Err(error) if error.kind() == io::ErrorKind::TimedOut => Ok(true),
            Err(error) => Err(error),
        }
    }

    fn observe(&self, timeout: std::time::Duration) -> io::Result<()> {
        #[cfg(target_os="windows")]
        {
            use windows::Win32::{Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT}, System::Threading::WaitForSingleObject};
            match unsafe { WaitForSingleObject(self.handle, timeout.as_millis().min(u32::MAX as u128 - 1) as u32) } {
                WAIT_OBJECT_0 => Ok(()),
                WAIT_TIMEOUT => Err(io::Error::new(io::ErrorKind::TimedOut,"ServiceProcessExitUnconfirmed")),
                _ => Err(io::Error::last_os_error()),
            }
        }
        #[cfg(target_os="linux")]
        {
            use std::os::fd::AsRawFd;
            let deadline = std::time::Instant::now() + timeout;
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                let mut descriptor = libc::pollfd { fd: self.pidfd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                let result = unsafe { libc::poll(&mut descriptor, 1, remaining.as_millis().min(i32::MAX as u128) as i32) };
                if result > 0 && descriptor.revents & (libc::POLLIN | libc::POLLHUP) != 0 { return Ok(()); }
                if result < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::Interrupted && std::time::Instant::now() < deadline { continue; }
                    return Err(error);
                }
                return Err(io::Error::new(io::ErrorKind::TimedOut,"ServiceProcessExitUnconfirmed"));
            }
        }
    }
}

#[cfg(target_os="windows")]
fn process_image(pid: u32) -> io::Result<String> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows::{core::PWSTR, Win32::{Foundation::HANDLE, System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION}}};
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(io::Error::other)? };
    let process = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut image = vec![0u16; 32768];
    let mut length = image.len() as u32;
    unsafe { QueryFullProcessImageNameW(HANDLE(process.as_raw_handle()), PROCESS_NAME_WIN32, PWSTR(image.as_mut_ptr()), &mut length).map_err(io::Error::other)?; }
    String::from_utf16(&image[..length as usize]).map_err(io::Error::other)
}
#[cfg(target_os="windows")]
impl Drop for ProcessExit { fn drop(&mut self) { unsafe { let _ = windows::Win32::Foundation::CloseHandle(self.handle); } } }
pub struct BackgroundChild {
    #[cfg(target_os="linux")]
    child: std::process::Child,
    #[cfg(target_os="windows")]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(target_os="windows")]
    pid: u32,
}
// The owned process handle may move to a lifecycle worker; it is closed once.
#[cfg(target_os="windows")]
unsafe impl Send for BackgroundChild {}
impl BackgroundChild {
    pub fn id(&self) -> u32 {
        #[cfg(target_os="linux")] { self.child.id() }
        #[cfg(target_os="windows")] { self.pid }
    }
    pub fn try_wait(&mut self) -> io::Result<Option<()>> {
        #[cfg(target_os="linux")] { self.child.try_wait().map(|status|status.map(|_|())) }
        #[cfg(target_os="windows")]
        {
            use windows::Win32::{Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT}, System::Threading::WaitForSingleObject};
            match unsafe { WaitForSingleObject(self.handle, 0) } {
                WAIT_OBJECT_0 => Ok(Some(())), WAIT_TIMEOUT => Ok(None), _ => Err(io::Error::last_os_error()),
            }
        }
    }
    pub fn kill(&mut self) -> io::Result<()> {
        #[cfg(target_os="linux")] { self.child.kill() }
        #[cfg(target_os="windows")] { unsafe { windows::Win32::System::Threading::TerminateProcess(self.handle, 1).map_err(io::Error::other) } }
    }
    pub fn wait(&mut self) -> io::Result<()> {
        #[cfg(target_os="linux")] { self.child.wait().map(|_|()) }
        #[cfg(target_os="windows")]
        {
            use windows::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject};
            if unsafe { WaitForSingleObject(self.handle, u32::MAX) } == WAIT_OBJECT_0 { Ok(()) } else { Err(io::Error::last_os_error()) }
        }
    }
}
#[cfg(target_os="windows")]
impl Drop for BackgroundChild { fn drop(&mut self) { unsafe { let _ = windows::Win32::Foundation::CloseHandle(self.handle); } } }

#[cfg(target_os="windows")]
fn quoted(argument: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let mut value = vec![b'"' as u16];
    let mut slashes = 0;
    for unit in argument.encode_wide() {
        if unit == 0 { return Err(io::Error::new(io::ErrorKind::InvalidInput,"InvalidHelperArgument")); }
        if unit == b'\\' as u16 { slashes += 1; continue; }
        value.extend(std::iter::repeat_n(b'\\' as u16, slashes * if unit == b'"' as u16 { 2 } else { 1 }));
        slashes = 0;
        if unit == b'"' as u16 { value.push(b'\\' as u16); }
        value.push(unit);
    }
    value.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    value.push(b'"' as u16);
    Ok(value)
}

pub fn spawn_background(executable: &Path, arguments: &[OsString]) -> io::Result<BackgroundChild> {
    #[cfg(target_os="windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{core::{PCWSTR, PWSTR}, Win32::{Foundation::CloseHandle, System::Threading::{CreateProcessW, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW}}};
        let application: Vec<u16> = executable.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut command = quoted(executable.as_os_str())?;
        for argument in arguments { command.push(b' ' as u16); command.extend(quoted(argument)?); }
        command.push(0);
        let startup = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
        let mut process = PROCESS_INFORMATION::default();
        // Inheriting even unrelated pipe handles keeps a parent CLI's redirected
        // stdout alive after that CLI exits. This service inherits no handles.
        unsafe { CreateProcessW(PCWSTR(application.as_ptr()), Some(PWSTR(command.as_mut_ptr())), None, None, false, CREATE_NO_WINDOW, None, None, &startup, &mut process).map_err(io::Error::other)?; }
        unsafe { let _ = CloseHandle(process.hThread); }
        return Ok(BackgroundChild { handle: process.hProcess, pid: process.dwProcessId });
    }
    #[cfg(target_os="linux")]
    {
    let mut command = Command::new(executable);
    command.args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    Ok(BackgroundChild { child: command.spawn()? })
    }
}

/// Query a known sibling helper with bounded output and lifetime. Only the
/// exact child created here is terminated on timeout or excessive output.
pub fn inspect_helper(executable: &Path, arguments: &[&str], timeout: std::time::Duration, maximum: usize) -> io::Result<Vec<u8>> {
    inspect_command(Command::new(executable),arguments,timeout,maximum)
}
/// A managed Surface inherits only the explicit runtime environment, including
/// its metadata-only probe. Grants never travel through environment variables.
pub fn inspect_private_helper(executable:&Path,arguments:&[&str],timeout:std::time::Duration,maximum:usize,environment:&[(OsString,OsString)])->io::Result<Vec<u8>> {
    let mut command = Command::new(executable);
    command.env_clear().envs(environment.iter().cloned());
    inspect_command(command,arguments,timeout,maximum)
}
fn inspect_command(mut command:Command,arguments:&[&str],timeout:std::time::Duration,maximum:usize)->io::Result<Vec<u8>> {
    command.args(arguments).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(target_os="windows")]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let mut child = command.spawn()?;
    let output = child.stdout.take().ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;
    let (sent, received) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = output.take(maximum as u64 + 1).read_to_end(&mut bytes).map(|_|bytes);
        let _ = sent.send(result);
    });
    let deadline = std::time::Instant::now() + timeout;
    let mut bytes = None;
    loop {
        if bytes.is_none() {
            if let Ok(result) = received.try_recv() {
                match result {
                    Ok(value) if value.len() <= maximum => bytes = Some(value),
                    _ => { let _=child.kill(); let _=child.wait(); return Err(io::Error::new(io::ErrorKind::InvalidData,"HelperOutputRejected")); }
                }
            }
        }
        if let Some(status) = child.try_wait()? {
            if !status.success() { return Err(io::Error::new(io::ErrorKind::Other,"HelperIdentityFailed")); }
            if let Some(bytes) = bytes { let _=reader.join(); return Ok(bytes); }
        }
        if std::time::Instant::now() >= deadline {
            let _=child.kill(); let _=child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut,"HelperIdentityUnconfirmed"));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn executable_identity_child() {
        if std::env::var_os("DZC_EXECUTABLE_IDENTITY_CHILD").is_some() {
            let mut byte = [0];
            let _ = std::io::stdin().read(&mut byte);
        }
    }

    #[test]
    fn executable_identity_excludes_own_child_but_accepts_another_program() {
        let peer = ProcessExit::open(&super::super::current_identity().unwrap()).unwrap();
        assert!(peer.shares_executable(std::process::id()).unwrap());
        let mut own = Command::new(std::env::current_exe().unwrap());
        own.args(["--exact", "ipc::process::identity_tests::executable_identity_child"])
            .env("DZC_EXECUTABLE_IDENTITY_CHILD", "1");
        #[cfg(target_os="windows")]
        let mut external = { let mut command = Command::new("cmd.exe"); command.args(["/D", "/Q", "/C", "set /p DZC_IDENTITY_WAIT="]); command };
        #[cfg(target_os="linux")]
        let mut external = Command::new("/bin/cat");
        for (command, expected) in [(&mut own, true), (&mut external, false)] {
            command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
            #[cfg(target_os="windows")]
            { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
            let mut child = command.spawn().unwrap();
            let result = peer.shares_executable(child.id());
            drop(child.stdin.take());
            child.wait().unwrap();
            assert_eq!(result.unwrap(), expected);
        }
    }
}

#[cfg(all(test, target_os="windows"))]
mod tests {
    use super::*;
    #[test]
    fn background_arguments_quote_spaces_quotes_and_trailing_backslashes() {
        for (input, expected) in [
            ("", "\"\""), ("a b", "\"a b\""), ("a\"b", "\"a\\\"b\""),
            ("C:\\fixture path\\", "\"C:\\fixture path\\\\\""),
        ] {
            assert_eq!(String::from_utf16(&quoted(std::ffi::OsStr::new(input)).unwrap()).unwrap(), expected);
        }
        assert!(quoted(std::ffi::OsStr::new("a\0b")).is_err());
    }
}
