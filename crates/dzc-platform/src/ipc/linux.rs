use super::{denied, namespace, PeerIdentity};
use std::{fs, io, os::{fd::AsRawFd, unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt}}, path::{Path, PathBuf}, pin::Pin, task::{Context, Poll}, time::Duration};
use tokio::{io::{AsyncRead, AsyncWrite, ReadBuf}, net::{UnixListener, UnixStream}};

pub fn current_identity() -> io::Result<PeerIdentity> {
    // Native identity is independent of USER, HOME and any hello message.
    let uid = unsafe { libc::geteuid() };
    let session = std::env::var("WAYLAND_DISPLAY").or_else(|_| std::env::var("DISPLAY")).unwrap_or_else(|_| "headless".into());
    Ok(PeerIdentity { user: uid.to_string(), session, pid: Some(std::process::id()) })
}

fn private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 { return Err(denied()); }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Endpoint { path: PathBuf, identity: PeerIdentity }
impl Endpoint {
    pub fn for_current_session(data_directory: &Path) -> io::Result<Self> {
        let identity = current_identity()?;
        let root = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| data_directory.to_path_buf());
        let directory = root.join(format!("dzc-{}", namespace(data_directory, &identity)));
        Self::in_directory(&directory)
    }
    /// Explicit private directory for isolated profiles and transport tests.
    pub fn in_directory(directory: &Path) -> io::Result<Self> {
        private_directory(directory)?;
        let path = directory.join("host.sock");
        if path.as_os_str().as_encoded_bytes().len() > 100 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "local endpoint path too long")); }
        Ok(Self { path, identity: current_identity()? })
    }
    pub fn address(&self) -> &Path { &self.path }
    pub fn control_path(&self) -> PathBuf { self.path.with_extension("control.json") }
    pub async fn connect(&self, timeout: Duration) -> io::Result<Stream> {
        let stream = tokio::time::timeout(timeout, UnixStream::connect(&self.path)).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
        let peer = stream.peer_cred()?;
        if peer.uid().to_string() != self.identity.user { return Err(denied()); }
        let mut identity = self.identity.clone();
        identity.pid = peer.pid().and_then(|pid| u32::try_from(pid).ok());
        Ok(Stream(stream, identity))
    }
    pub fn bind(&self) -> io::Result<Listener> {
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(self.path.with_extension("lock"))?;
        let metadata = lock.metadata()?;
        if !metadata.is_file() || metadata.uid().to_string() != self.identity.user || metadata.mode() & 0o077 != 0 { return Err(denied()); }
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 { return Err(io::Error::from(io::ErrorKind::AddrInUse)); }
        // A stale socket is removed only after proving it is not accepting clients.
        if self.path.exists() {
            let metadata = fs::symlink_metadata(&self.path)?;
            use std::os::unix::fs::FileTypeExt;
            if !metadata.file_type().is_socket() || metadata.uid().to_string() != self.identity.user { return Err(denied()); }
            match std::os::unix::net::UnixStream::connect(&self.path) {
                Ok(_) => return Err(io::Error::from(io::ErrorKind::AddrInUse)),
                Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => fs::remove_file(&self.path)?,
                Err(error) => return Err(error),
            }
        }
        let listener = UnixListener::bind(&self.path)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        Ok(Listener { listener, endpoint: self.clone(), _lock: lock })
    }
}

pub struct Listener { listener: UnixListener, endpoint: Endpoint, _lock: fs::File }
impl Listener {
    pub async fn accept(&mut self) -> io::Result<Stream> {
        let (stream, _) = self.listener.accept().await?;
        let peer = stream.peer_cred()?;
        if peer.uid().to_string() != self.endpoint.identity.user { return Err(denied()); }
        let mut identity = self.endpoint.identity.clone();
        identity.pid = peer.pid().and_then(|pid| u32::try_from(pid).ok());
        Ok(Stream(stream, identity))
    }
}
impl Drop for Listener { fn drop(&mut self) { let _ = fs::remove_file(&self.endpoint.path); } }

pub struct Stream(UnixStream, PeerIdentity);
impl Stream {
    pub fn authenticate_after_read(&self) -> io::Result<PeerIdentity> { Ok(self.1.clone()) }
}
impl AsyncRead for Stream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buffer: &mut ReadBuf<'_>) -> Poll<io::Result<()>> { Pin::new(&mut self.0).poll_read(cx, buffer) }
}
impl AsyncWrite for Stream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> { Pin::new(&mut self.0).poll_write(cx, data) }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> { Pin::new(&mut self.0).poll_flush(cx) }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> { Pin::new(&mut self.0).poll_shutdown(cx) }
}
