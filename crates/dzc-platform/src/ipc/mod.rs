//! Current-user local transports. All native authentication stays in Platform.
use std::{io, path::Path, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{current_identity, Endpoint, Listener, Stream};
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{current_identity, Endpoint, Listener, Stream};
mod ownership;
pub use ownership::CaptureLease;
mod private_file;
pub use private_file::{read_private, write_private};
mod process;
pub use process::{spawn_background, inspect_helper, inspect_private_helper, ProcessExit, BackgroundChild};

/// Produced only after an operating-system identity check, never deserialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    user: String,
    session: String,
    pid: Option<u32>,
}

impl PeerIdentity {
    pub fn user(&self) -> &str { &self.user }
    /// Windows: verified token session ID. Linux: the local endpoint's graphical
    /// scope label, not an OS-authenticated claim about the peer's session.
    pub fn session(&self) -> &str { &self.session }
    pub fn pid(&self) -> Option<u32> { self.pid }
}

pub(crate) fn denied() -> io::Error { io::Error::from(io::ErrorKind::PermissionDenied) }

pub(crate) fn namespace(path: &Path, identity: &PeerIdentity) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(path.as_os_str().to_string_lossy().as_bytes());
    digest.update([0]);
    digest.update(identity.user.as_bytes());
    digest.update([0]);
    digest.update(identity.session.as_bytes());
    format!("{:x}", digest.finalize())[..24].to_owned()
}

/// A 4-byte big-endian byte count precedes one UTF-8 JSON value. The allocation
/// occurs only after checking the untrusted count. A deadline covers the entire
/// frame, including peers that never finish their header or payload.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R, timeout: Duration) -> io::Result<Vec<u8>> {
    tokio::time::timeout(timeout, async {
        let length = reader.read_u32().await? as usize;
        if length == 0 || length > dzc_protocol::wire::MAX_FRAME_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid local frame length"));
        }
        let mut payload = vec![0; length];
        reader.read_exact(&mut payload).await?;
        std::str::from_utf8(&payload).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid local UTF-8"))?;
        Ok(payload)
    }).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, payload: &[u8], timeout: Duration) -> io::Result<()> {
    if payload.is_empty() || payload.len() > dzc_protocol::wire::MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid local frame length"));
    }
    tokio::time::timeout(timeout, async {
        writer.write_u32(payload.len() as u32).await?;
        writer.write_all(payload).await?;
        writer.flush().await
    }).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_frames_and_slow_or_oversize_peers_are_bounded() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let (mut a, mut b) = tokio::io::duplex(32);
            let writer = tokio::spawn(async move {
                for byte in [0, 0, 0, 2, b'{', b'}'] { a.write_all(&[byte]).await.unwrap(); }
            });
            assert_eq!(read_frame(&mut b, Duration::from_secs(1)).await.unwrap(), b"{}");
            writer.await.unwrap();
            let (mut a, mut b) = tokio::io::duplex(32);
            a.write_all(&[0xff; 4]).await.unwrap();
            assert_eq!(read_frame(&mut b, Duration::from_secs(1)).await.unwrap_err().kind(), io::ErrorKind::InvalidData);
            let (_a, mut b) = tokio::io::duplex(32);
            assert_eq!(read_frame(&mut b, Duration::from_millis(10)).await.unwrap_err().kind(), io::ErrorKind::TimedOut);
        });
    }

    #[test]
    fn private_native_endpoint_authenticates_and_releases_ownership() {
        let directory = std::env::temp_dir().join(format!("dzc-ipc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        #[cfg(target_os = "linux")]
        { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap(); }
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let endpoint = Endpoint::in_directory(&directory).unwrap();
            let mut listener = endpoint.bind().unwrap();
            assert!(endpoint.bind().is_err());
            let client = tokio::spawn(async move {
                let mut stream = endpoint.connect(Duration::from_secs(2)).await.unwrap();
                write_frame(&mut stream, b"{}", Duration::from_secs(2)).await.unwrap();
                assert_eq!(read_frame(&mut stream, Duration::from_secs(2)).await.unwrap(), b"{}");
            });
            let mut stream = listener.accept().await.unwrap();
            assert_eq!(read_frame(&mut stream, Duration::from_secs(2)).await.unwrap(), b"{}");
            let peer = stream.authenticate_after_read().unwrap();
            assert_eq!(peer.user(), current_identity().unwrap().user());
            write_frame(&mut stream, b"{}", Duration::from_secs(2)).await.unwrap();
            client.await.unwrap();
            drop(stream);
            drop(listener);
            let endpoint = Endpoint::in_directory(&directory).unwrap();
            // Tokio may finish cancelling registered overlapped operations on
            // the next reactor turn after the last pipe handle is dropped.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            loop {
                if let Ok(rebound) = endpoint.bind() { drop(rebound); break; }
                assert!(tokio::time::Instant::now() < deadline, "native endpoint was not released");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        std::fs::remove_dir_all(directory).unwrap();
    }
}
