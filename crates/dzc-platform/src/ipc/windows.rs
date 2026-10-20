use super::{denied, namespace, PeerIdentity};
use std::{io, os::windows::io::AsRawHandle, path::{Path, PathBuf}, pin::Pin, task::{Context, Poll}, time::Duration};
use tokio::{io::{AsyncRead, AsyncWrite, ReadBuf}, net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions}};
use windows::{core::{PCWSTR, PWSTR}, Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
    Security::{Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1}, GetTokenInformation, RevertToSelf, TokenSessionId, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER},
    System::{Pipes::{GetNamedPipeServerProcessId, ImpersonateNamedPipeClient}, Threading::{GetCurrentProcess, GetCurrentThread, OpenProcess, OpenProcessToken, OpenThreadToken, PROCESS_QUERY_LIMITED_INFORMATION}},
}};

fn native_error(error: windows::core::Error) -> io::Error { io::Error::new(io::ErrorKind::PermissionDenied, error) }
struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle { fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } } }
struct LocalAllocation(*mut std::ffi::c_void);
impl Drop for LocalAllocation { fn drop(&mut self) { unsafe { let _ = LocalFree(Some(HLOCAL(self.0))); } } }

fn token_identity(token: HANDLE) -> io::Result<PeerIdentity> {
    unsafe {
        let mut length = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut length);
        if length == 0 || length > 65_536 { return Err(denied()); }
        let mut storage = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(token, TokenUser, Some(storage.as_mut_ptr().cast()), length, &mut length).map_err(native_error)?;
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut sid = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid).map_err(native_error)?;
        let allocation = LocalAllocation(sid.0.cast());
        let user = sid.to_string().map_err(|_| denied())?;
        drop(allocation);
        let mut session = 0u32;
        GetTokenInformation(token, TokenSessionId, Some((&mut session as *mut u32).cast()), 4, &mut length).map_err(native_error)?;
        Ok(PeerIdentity { user, session: session.to_string(), pid: None })
    }
}

fn process_identity(process: HANDLE) -> io::Result<PeerIdentity> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(native_error)?; }
    let token = OwnedHandle(token);
    token_identity(token.0)
}

pub fn current_identity() -> io::Result<PeerIdentity> {
    let mut identity = process_identity(unsafe { GetCurrentProcess() })?;
    identity.pid = Some(std::process::id());
    Ok(identity)
}

fn same_user_session(expected: &PeerIdentity, peer: &PeerIdentity) -> io::Result<()> {
    if expected.user != peer.user || expected.session != peer.session { return Err(denied()); }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Endpoint { address: PathBuf, identity: PeerIdentity }
impl Endpoint {
    pub fn for_current_session(data_directory: &Path) -> io::Result<Self> {
        let identity = current_identity()?;
        let address = PathBuf::from(format!(r"\\.\pipe\dezirclip-{}-{}-{}", identity.user, identity.session, namespace(data_directory, &identity)));
        Ok(Self { address, identity })
    }
    pub fn in_directory(directory: &Path) -> io::Result<Self> { Self::for_current_session(directory) }
    pub fn address(&self) -> &Path { &self.address }
    pub async fn connect(&self, timeout: Duration) -> io::Result<Stream> {
        let deadline = tokio::time::Instant::now() + timeout;
        let pipe = loop {
            match ClientOptions::new().open(&self.address) {
                Ok(pipe) => break pipe,
                Err(error) if error.raw_os_error() == Some(231) && tokio::time::Instant::now() < deadline => tokio::time::sleep(Duration::from_millis(10)).await,
                Err(error) => return Err(error),
            }
        };
        // Verify the listening process as well as the server-side client token.
        let mut pid = 0;
        unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut pid).map_err(native_error)?; }
        let process = OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(native_error)? });
        let mut peer = process_identity(process.0)?;
        same_user_session(&self.identity, &peer)?;
        peer.pid = Some(pid);
        Ok(Stream { pipe: Pipe::Client(pipe), identity: peer })
    }
    pub fn bind(&self) -> io::Result<Listener> {
        Ok(Listener { next: self.create(true)?, endpoint: self.clone() })
    }
    fn create(&self, first: bool) -> io::Result<NamedPipeServer> {
        // Protected DACL: only this user may connect; remote clients are rejected
        // by the kernel. Client token identity is checked again after the hello.
        let descriptor: Vec<u16> = format!("D:P(A;;GA;;;{})", self.identity.user).encode_utf16().chain(Some(0)).collect();
        let mut security = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(descriptor.as_ptr()), SDDL_REVISION_1, &mut security, None).map_err(native_error)?; }
        let _allocation = LocalAllocation(security.0);
        let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: security.0, bInheritHandle: false.into() };
        unsafe { ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).max_instances(32)
            .create_with_security_attributes_raw(&self.address, (&attributes as *const SECURITY_ATTRIBUTES).cast_mut().cast()) }
    }
}

pub struct Listener { next: NamedPipeServer, endpoint: Endpoint }
impl Listener {
    pub async fn accept(&mut self) -> io::Result<Stream> {
        self.next.connect().await?;
        let next = self.endpoint.create(false)?;
        let pipe = std::mem::replace(&mut self.next, next);
        Ok(Stream { pipe: Pipe::Server(pipe), identity: self.endpoint.identity.clone() })
    }
}

enum Pipe { Server(NamedPipeServer), Client(NamedPipeClient) }
pub struct Stream { pipe: Pipe, identity: PeerIdentity }
impl Stream {
    /// Called synchronously after reading the hello frame. Impersonation never
    /// crosses an await or invokes peer-provided code; every exit reverts it.
    pub fn authenticate_after_read(&self) -> io::Result<PeerIdentity> {
        let Pipe::Server(pipe) = &self.pipe else { return Ok(self.identity.clone()); };
        struct Revert;
        impl Drop for Revert { fn drop(&mut self) { if unsafe { RevertToSelf() }.is_err() { std::process::abort(); } } }
        unsafe { ImpersonateNamedPipeClient(HANDLE(pipe.as_raw_handle())).map_err(native_error)?; }
        let _revert = Revert;
        let mut token = HANDLE::default();
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token).map_err(native_error)?; }
        let token = OwnedHandle(token);
        let peer = token_identity(token.0)?;
        same_user_session(&self.identity, &peer)?;
        Ok(peer)
    }
}
impl AsyncRead for Stream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buffer: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match &mut self.pipe { Pipe::Server(pipe) => Pin::new(pipe).poll_read(cx, buffer), Pipe::Client(pipe) => Pin::new(pipe).poll_read(cx, buffer) }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        match &mut self.pipe { Pipe::Server(pipe) => Pin::new(pipe).poll_write(cx, data), Pipe::Client(pipe) => Pin::new(pipe).poll_write(cx, data) }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.pipe { Pipe::Server(pipe) => Pin::new(pipe).poll_flush(cx), Pipe::Client(pipe) => Pin::new(pipe).poll_flush(cx) }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.pipe { Pipe::Server(pipe) => Pin::new(pipe).poll_shutdown(cx), Pipe::Client(pipe) => Pin::new(pipe).poll_shutdown(cx) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_comparison_does_not_accept_another_user_or_session() {
        let current = current_identity().unwrap();
        let mut other = current.clone(); other.user.push('0');
        assert!(same_user_session(&current, &other).is_err());
        let mut other = current.clone(); other.session.push('0');
        assert!(same_user_session(&current, &other).is_err());
    }
}
