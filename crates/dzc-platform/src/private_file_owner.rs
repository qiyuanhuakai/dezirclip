//! A separate descriptor table prevents old export URIs from aliasing later Host exports.
use std::{
    collections::HashSet,
    fs::File,
    os::fd::AsRawFd,
    sync::{Mutex, OnceLock},
};

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
        // After fork the child invokes only syscalls, never inherited Rust locks or destructors.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err("PrivateExportRejected".into());
        }
        if pid == 0 {
            unsafe {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0
                    || libc::getppid() != parent
                {
                    libc::_exit(1);
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
                let ready = 1u8;
                if libc::write(ready_write.as_raw_fd(), (&ready as *const u8).cast(), 1) != 1 {
                    libc::_exit(1);
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
        drop(control_read);
        drop(ready_write);
        let process = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
        if process < 0 {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            return Err("PrivateExportRejected".into());
        }
        let owner = Self {
            pid,
            control: Some(control_write),
            process: unsafe { File::from_raw_fd(process as i32) },
        };
        let mut wait = libc::pollfd {
            fd: ready_read.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let mut ready = 0u8;
        let started = unsafe {
            libc::poll(&mut wait, 1, 2_000) > 0
                && libc::read(ready_read.as_raw_fd(), (&mut ready as *mut u8).cast(), 1) == 1
                && ready == 1
        };
        if !started
            || !USED_PIDS
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| "PrivateExportRejected")?
                .insert(pid)
        {
            // Files are still empty: even a recycled PID cannot expose a later plaintext export.
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
                    libc::waitpid(self.pid, std::ptr::null_mut(), libc::WNOHANG);
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
