use super::WaylandError;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Instant;

pub(super) fn check_cancel(cancel: Option<&UnixStream>) -> Result<(), WaylandError> {
    if let Some(cancel) = cancel {
        let mut fd = libc::pollfd {
            fd: cancel.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // A zero-time check is only made while processing an active operation.
        let ready = unsafe { libc::poll(&mut fd, 1, 0) };
        if ready < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(WaylandError::Disconnected);
        }
        if ready > 0 && fd.revents != 0 {
            return Err(WaylandError::Cancelled);
        }
    }
    Ok(())
}

pub(crate) fn wait(
    fd: i32,
    events: i16,
    cancel: Option<&UnixStream>,
    deadline: Instant,
) -> Result<(), WaylandError> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(WaylandError::TimedOut)?;
        let mut fds = [
            libc::pollfd {
                fd,
                events,
                revents: 0,
            },
            libc::pollfd {
                fd: cancel.map(AsRawFd::as_raw_fd).unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ms = remaining.as_millis().min(i32::MAX as u128).max(1) as i32;
        // Both descriptors are owned for the complete poll; no user-provided pointer is used.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
        if result < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(WaylandError::Disconnected);
        }
        if result == 0 {
            return Err(WaylandError::TimedOut);
        }
        if fds[1].revents != 0 {
            return Err(WaylandError::Cancelled);
        }
        if fds[0].revents & libc::POLLNVAL != 0 {
            return Err(WaylandError::Disconnected);
        }
        if fds[0].revents & (events | libc::POLLHUP | libc::POLLERR) != 0 {
            return Ok(());
        }
    }
}

pub(super) fn pipe() -> Result<(OwnedFd, OwnedFd), WaylandError> {
    let mut fds = [-1; 2];
    // On success pipe2 transfers two distinct owned descriptors to this function.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return Err(WaylandError::TransferFailed);
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

pub(super) fn receive(
    fd: OwnedFd,
    limit: usize,
    cancel: Option<&UnixStream>,
    deadline: Instant,
) -> Result<Vec<u8>, WaylandError> {
    let mut input = std::fs::File::from(fd);
    let mut result = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        check_cancel(cancel)?;
        match input.read(&mut buffer) {
            Ok(0) => return Ok(result),
            Ok(count) => {
                if result.len().saturating_add(count) > limit {
                    return Err(WaylandError::TransferBudgetExceeded);
                }
                result.extend_from_slice(&buffer[..count]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                wait(input.as_raw_fd(), libc::POLLIN, cancel, deadline)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
            Err(_) => return Err(WaylandError::TransferFailed),
        }
        if Instant::now() >= deadline {
            return Err(WaylandError::TimedOut);
        }
    }
}

pub(super) fn send(
    fd: OwnedFd,
    bytes: &[u8],
    cancel: Option<&UnixStream>,
    deadline: Instant,
) -> Result<(), WaylandError> {
    // The compositor gives us an owned pipe endpoint. Preserve its status flags.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(WaylandError::TransferFailed);
    }
    let mut output = std::fs::File::from(fd);
    let mut offset = 0;
    while offset < bytes.len() {
        check_cancel(cancel)?;
        match output.write(&bytes[offset..]) {
            Ok(0) => return Err(WaylandError::TransferFailed),
            Ok(count) => offset += count,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                wait(output.as_raw_fd(), libc::POLLOUT, cancel, deadline)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
            Err(_) => return Err(WaylandError::TransferFailed),
        }
        if Instant::now() >= deadline {
            return Err(WaylandError::TimedOut);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn bounded_pipe_read_rejects_over_budget_and_silent_owners() {
        let (read, write) = pipe().unwrap();
        send(
            write,
            b"synthetic",
            None,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(
            receive(read, 3, None, Instant::now() + Duration::from_secs(1)),
            Err(WaylandError::TransferBudgetExceeded)
        );
        let (read, _write) = pipe().unwrap();
        assert_eq!(
            receive(read, 64, None, Instant::now() + Duration::from_millis(10)),
            Err(WaylandError::TimedOut)
        );
    }
    #[test]
    fn cancellation_interrupts_transfer_without_polling() {
        let (read, _write) = pipe().unwrap();
        let (cancel, mut trigger) = UnixStream::pair().unwrap();
        trigger.write_all(&[1]).unwrap();
        assert_eq!(
            receive(
                read,
                64,
                Some(&cancel),
                Instant::now() + Duration::from_secs(1)
            ),
            Err(WaylandError::Cancelled)
        );
    }
}
