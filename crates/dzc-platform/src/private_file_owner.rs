//! A separate descriptor table prevents old export URIs from aliasing later Host exports.
use std::{
    collections::HashSet,
    fs::File,
    os::fd::AsRawFd,
    sync::{Mutex, OnceLock},
};

static FORK_WINDOW: Mutex<()> = Mutex::new(());

static USED_PIDS: OnceLock<Mutex<HashSet<libc::pid_t>>> = OnceLock::new();

pub(super) struct Owner {
    pid: libc::pid_t,
    control: Option<File>,
    process: File,
}

impl Owner {
    pub(super) fn pid(&self) -> libc::pid_t {
        self.pid
    }

    pub(super) fn start(files: &[File]) -> Result<Self, String> {
        Self::start_checked(files, None, |pid| {
            USED_PIDS
                .get_or_init(Default::default)
                .lock()
                .map(|mut used| used.insert(pid))
                .unwrap_or(false)
        })
    }

    fn start_checked(
        files: &[File],
        before_close: Option<&dyn Fn(libc::pid_t)>,
        approve: impl Fn(libc::pid_t) -> bool,
    ) -> Result<Self, String> {
        use std::os::fd::FromRawFd;
        fn pipe() -> Result<[File; 2], String> {
            let mut fds = [-1; 2];
            if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } < 0 {
                return Err("PrivateExportRejected".into());
            }
            Ok(unsafe { [File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])] })
        }
        let [control_read, control_write] = pipe()?;
        let [ready_read, ready_write] = pipe()?;
        let mut keep: Vec<u32> = files.iter().map(|file| file.as_raw_fd() as u32).collect();
        keep.extend([
            control_read.as_raw_fd() as u32,
            ready_write.as_raw_fd() as u32,
        ]);
        keep.sort_unstable();
        let parent = unsafe { libc::getpid() };
        let fork_window = FORK_WINDOW.lock().map_err(|_| "PrivateExportRejected")?;
        let mut dumpability = ForkPrivacy::begin()?;
        // The child inherits non-dumpability from birth, including before its first instruction.
        // After fork it invokes only syscalls, never inherited Rust locks or destructors.
        let mut process_fd = -1i32;
        // Raw clone has fork semantics here (no shared VM/FD table). CLONE_PIDFD atomically
        // returns the original child's identity; no numeric-PID lookup/reap race is possible.
        // x86_64 and aarch64 use parent_tid as the third argument; unused TLS/child_tid are zero.
        let pid = unsafe {
            libc::syscall(
                libc::SYS_clone,
                libc::CLONE_PIDFD | libc::SIGCHLD,
                0usize,
                &mut process_fd as *mut i32,
                0usize,
                0usize,
            )
        } as libc::pid_t;
        if pid == 0 {
            unsafe {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0
                    || libc::getppid() != parent
                {
                    libc::_exit(1);
                }
                if before_close.is_some() {
                    if !child_send(ready_write.as_raw_fd(), 0)
                        || !receive(control_read.as_raw_fd(), 0)
                    {
                        libc::_exit(1);
                    }
                }
                let mut first = 0u32;
                for &fd in &keep {
                    if first < fd && libc::syscall(libc::SYS_close_range, first, fd - 1, 0u32) < 0 {
                        libc::_exit(1);
                    }
                    first = fd + 1;
                }
                if libc::syscall(libc::SYS_close_range, first, u32::MAX, 0u32) < 0 {
                    libc::_exit(1);
                }
                if !child_send(ready_write.as_raw_fd(), 1) || !receive(control_read.as_raw_fd(), 1)
                {
                    libc::_exit(1);
                }
                if libc::prctl(libc::PR_SET_DUMPABLE, 1) < 0
                    || !child_send(ready_write.as_raw_fd(), 2)
                {
                    libc::_exit(1);
                }
                if before_close.is_some() {
                    libc::poll(std::ptr::null_mut(), 0, 10);
                }
                libc::close(ready_write.as_raw_fd());
                let mut wait = libc::pollfd {
                    fd: control_read.as_raw_fd(),
                    events: libc::POLLIN | libc::POLLHUP,
                    revents: 0,
                };
                // EOF, revocation, or a bounded lifetime all close the complete owner FD table.
                libc::poll(&mut wait, 1, 300_000);
                libc::_exit(0);
            }
        }
        let restored = dumpability.restore();
        drop(dumpability);
        drop(fork_window);
        if pid < 0 {
            return Err("PrivateExportRejected".into());
        }
        drop(control_read);
        drop(ready_write);
        if process_fd < 0 {
            return Err("PrivateExportRejected".into());
        }
        let mut owner = Self {
            pid,
            control: Some(control_write),
            process: unsafe { File::from_raw_fd(process_fd) },
        };
        restored?;
        if let Some(probe) = before_close {
            if !unsafe { receive(ready_read.as_raw_fd(), 0) } {
                return Err("PrivateExportRejected".into());
            }
            probe(pid);
            use std::io::Write;
            owner
                .control
                .as_mut()
                .ok_or("PrivateExportRejected")?
                .write_all(&[0])
                .map_err(|_| "PrivateExportRejected")?;
        }
        if !unsafe { receive(ready_read.as_raw_fd(), 1) } || !approve(pid) {
            // The child remains non-dumpable and the member files remain empty on refusal.
            return Err("PrivateExportRejected".into());
        }
        use std::io::Write;
        owner
            .control
            .as_mut()
            .ok_or("PrivateExportRejected")?
            .write_all(&[1])
            .map_err(|_| "PrivateExportRejected")?;
        if !unsafe { receive(ready_read.as_raw_fd(), 2) && receive_eof(ready_read.as_raw_fd()) } {
            return Err("PrivateExportRejected".into());
        }
        Ok(owner)
    }
}

impl Owner {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.control.take();
        // pidfd addresses the original process even if another SIGCHLD handler has reaped it.
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.process.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0u32,
            );
        }
        let mut wait = libc::pollfd {
            fd: self.process.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if unsafe { libc::poll(&mut wait, 1, 20) } > 0 && wait.revents & libc::POLLIN != 0 {
                unsafe {
                    let mut status: libc::siginfo_t = std::mem::zeroed();
                    libc::waitid(
                        libc::P_PIDFD,
                        self.process.as_raw_fd() as libc::id_t,
                        &mut status,
                        libc::WEXITED | libc::WNOHANG,
                    );
                }
                return Ok(());
            }
        }
        Err("PrivateExportRevocationUnconfirmed".into())
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

// The application never changes credentials or dumpability elsewhere. This serialized interval
// only tightens peer access and restores the exact accepted initial state before returning.
struct ForkPrivacy {
    previous_signals: libc::sigset_t,
    changed_dumpability: bool,
    restored: bool,
}
impl ForkPrivacy {
    fn begin() -> Result<Self, String> {
        let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut previous_signals: libc::sigset_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigfillset(&mut blocked) } != 0
            || unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &blocked, &mut previous_signals) }
                != 0
        {
            return Err("PrivateExportRejected".into());
        }
        let mut guard = Self {
            previous_signals,
            changed_dumpability: false,
            restored: false,
        };
        if unsafe { libc::prctl(libc::PR_GET_DUMPABLE) } != 1
            || unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } < 0
        {
            return Err("PrivateExportRejected".into());
        }
        guard.changed_dumpability = true;
        Ok(guard)
    }
    fn restore(&mut self) -> Result<(), String> {
        if self.restored {
            return Ok(());
        }
        let dumpability =
            !self.changed_dumpability || unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 1) } == 0;
        let signals = unsafe {
            libc::pthread_sigmask(
                libc::SIG_SETMASK,
                &self.previous_signals,
                std::ptr::null_mut(),
            )
        } == 0;
        self.restored = dumpability && signals;
        if self.restored {
            Ok(())
        } else {
            Err("PrivateExportRejected".into())
        }
    }
}
impl Drop for ForkPrivacy {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

unsafe fn child_send(fd: i32, value: u8) -> bool {
    unsafe { libc::write(fd, (&value as *const u8).cast(), 1) == 1 }
}
unsafe fn receive(fd: i32, expected: u8) -> bool {
    let mut wait = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let mut value = 255u8;
    unsafe {
        libc::poll(&mut wait, 1, 2_000) > 0
            && libc::read(fd, (&mut value as *mut u8).cast(), 1) == 1
            && value == expected
    }
}

unsafe fn receive_eof(fd: i32) -> bool {
    let mut wait = libc::pollfd {
        fd,
        events: libc::POLLIN | libc::POLLHUP,
        revents: 0,
    };
    let mut byte = 0u8;
    unsafe {
        libc::poll(&mut wait, 1, 2_000) > 0 && libc::read(fd, (&mut byte as *mut u8).cast(), 1) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_parent_restored() {
        let _window = FORK_WINDOW.lock().unwrap();
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE) }, 1);
    }
    fn empty_member() -> File {
        use std::os::fd::FromRawFd;
        let fd = unsafe { libc::memfd_create(c"synthetic-owner-test".as_ptr(), libc::MFD_CLOEXEC) };
        assert!(fd >= 0);
        unsafe { File::from_raw_fd(fd) }
    }
    #[test]
    fn peer_access_is_denied_before_inherited_descriptors_are_closed() {
        let previous =
            crate::private_files::Exports::create(vec![dzc_core::ports::ClipboardFile {
                name: "active-C.txt".into(),
                bytes: b"synthetic active export C".to_vec(),
            }])
            .unwrap();
        let unrelated = &previous.handles[0];
        let member = empty_member();
        let probe = |pid| {
            let path = format!("/proc/{pid}/fd/{}", unrelated.as_raw_fd());
            let error = File::open(&path).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(libc::EACCES));
            assert!(std::fs::read_link(path).is_err());
            let peer = std::process::Command::new("/bin/cat")
                .arg(format!("/proc/{pid}/fd/{}", unrelated.as_raw_fd()))
                .output()
                .unwrap();
            assert!(!peer.status.success());
            assert!(peer.stdout.is_empty());
            assert_parent_restored();
        };
        let owner =
            Owner::start_checked(std::slice::from_ref(&member), Some(&probe), |_| true).unwrap();
        assert_parent_restored();
        assert!(File::open(format!("/proc/{}/fd/{}", owner.pid(), member.as_raw_fd())).is_ok());
        assert!(File::open(format!(
            "/proc/{}/fd/{}",
            owner.pid(),
            unrelated.as_raw_fd()
        ))
        .is_err());
    }
    #[test]
    fn fork_privacy_guard_restores_parent_on_unwind() {
        let _window = FORK_WINDOW.lock().unwrap();
        let mut before: libc::sigset_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut before) },
            0
        );
        let result = std::panic::catch_unwind(|| {
            let _privacy = ForkPrivacy::begin().unwrap();
            assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE) }, 0);
            panic!("synthetic fork preparation failure");
        });
        assert!(result.is_err());
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE) }, 1);
        let mut after: libc::sigset_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut after) },
            0
        );
        for signal in 1..=64 {
            assert_eq!(unsafe { libc::sigismember(&before, signal) }, unsafe {
                libc::sigismember(&after, signal)
            });
        }
    }
    #[test]
    fn an_already_non_dumpable_parent_is_not_weakened() {
        let _window = FORK_WINDOW.lock().unwrap();
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                unsafe {
                    libc::prctl(libc::PR_SET_DUMPABLE, 1);
                }
            }
        }
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE) }, 1);
        let _reset = Reset;
        assert_eq!(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) }, 0);
        assert!(ForkPrivacy::begin().is_err());
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE) }, 0);
    }
    #[test]
    fn rejected_identity_never_becomes_peer_readable_and_restores_parent_state() {
        let member = empty_member();
        let seen = std::cell::Cell::new(None);
        let probe = |pid| {
            seen.set(Some(pid));
            assert!(File::open(format!("/proc/{pid}/fd/{}", member.as_raw_fd())).is_err());
        };
        assert!(
            Owner::start_checked(std::slice::from_ref(&member), Some(&probe), |_| false).is_err()
        );
        assert_eq!(member.metadata().unwrap().len(), 0);
        assert_parent_restored();
        assert!(!std::path::Path::new(&format!("/proc/{}/fd", seen.get().unwrap())).exists());
    }
}
