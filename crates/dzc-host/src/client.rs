use dzc_platform::ipc::{self, Endpoint, Stream};
use dzc_protocol::wire::{self, *};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{io, time::Duration};

/// Requests on one connection are correlated and serialized by ownership.
pub struct Client { stream: Stream, next: u64, pub hello: HelloResult, pub peer: ipc::PeerIdentity }
impl Client {
    pub async fn connect(endpoint: &Endpoint, role: ClientRole, grant: Option<String>) -> io::Result<Self> {
        let mut stream = endpoint.connect(Duration::from_secs(3)).await?;
        let peer = stream.authenticate_after_read()?;
        let hello = HelloParams { protocol_version: wire::VERSION, client_version: env!("CARGO_PKG_VERSION").into(), client_role: role,
            capabilities: vec![], required_capabilities: vec![], grant };
        let request = Request { request_id: "1".into(), method: Method::Hello, params: serde_json::to_value(hello)? };
        let value = exchange(&mut stream, request, "HelloResult").await?;
        let hello: HelloResult = serde_json::from_value(value)?;
        if hello.protocol_version.major != wire::VERSION.major { return Err(io::Error::new(io::ErrorKind::InvalidData, "ProtocolMismatch")); }
        Ok(Self { stream, next: 2, hello, peer })
    }
    pub async fn control(endpoint: &Endpoint, role: ClientRole) -> io::Result<Self> {
        // This is an explicit trusted local-client operation, never an automatic
        // consequence of a peer sending a desktop/CLI role in its hello.
        let credentials = ControlCredentials::read(endpoint)?;
        let grant = match role { ClientRole::Cli => credentials.cli, ClientRole::Desktop => credentials.desktop,
            _ => return Err(io::Error::from(io::ErrorKind::PermissionDenied)) };
        Self::connect(endpoint, role, Some(grant)).await
    }
    pub async fn call<T: DeserializeOwned>(&mut self, method: Method, parameters: impl Serialize, schema: &str) -> io::Result<T> {
        self.next = self.next.checked_add(1).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        let request = Request { request_id: self.next.to_string(), method, params: serde_json::to_value(parameters)? };
        serde_json::from_value(exchange(&mut self.stream, request, schema).await?).map_err(io::Error::other)
    }
}
async fn exchange(stream: &mut Stream, request: Request, schema: &str) -> io::Result<serde_json::Value> {
    let bytes = wire::encode("Request", &request).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "InvalidLocalRequest"))?;
    ipc::write_frame(stream, &bytes, Duration::from_secs(5)).await?;
    let bytes = ipc::read_frame(stream, Duration::from_secs(15)).await?;
    let response: Response = wire::decode("Response", &bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "InvalidLocalResponse"))?;
    if response.request_id != request.request_id { return Err(io::Error::new(io::ErrorKind::InvalidData, "ResponseCorrelationFailed")); }
    if let Some(error) = response.error { return Err(io::Error::new(io::ErrorKind::PermissionDenied, serde_json::to_string(&error.code)?)); }
    let value = response.result.ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    wire::validate(schema, &value).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "InvalidLocalResult"))?;
    Ok(value)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlCredentials { cli: String, desktop: String }
impl ControlCredentials {
    fn read(endpoint: &Endpoint) -> io::Result<Self> {
        serde_json::from_slice(&ipc::read_private(&endpoint.control_path())?).map_err(io::Error::other)
    }
}
/// The credential is installed after exclusive bind and removed only if still
/// equal to this Host's bytes. It grants no access on a different Host instance.
pub struct ControlFile { path: std::path::PathBuf, bytes: Vec<u8> }
impl ControlFile {
    pub fn publish(host: &crate::Host, endpoint: &Endpoint) -> io::Result<Self> {
        let cli = host.grants().issue_control(ClientRole::Cli).map_err(|_| io::Error::from(io::ErrorKind::Other))?;
        let desktop = host.grants().issue_control(ClientRole::Desktop).map_err(|_| io::Error::from(io::ErrorKind::Other))?;
        let bytes = serde_json::to_vec(&ControlCredentials { cli: cli.token().into(), desktop: desktop.token().into() })?;
        let path = endpoint.control_path();
        ipc::write_private(&path, &bytes)?;
        Ok(Self { path, bytes })
    }
}
impl Drop for ControlFile { fn drop(&mut self) {
    if ipc::read_private(&self.path).ok().as_deref() == Some(self.bytes.as_slice()) { let _ = std::fs::remove_file(&self.path); }
} }
