use dzc_platform::ipc::{self, Endpoint, Stream};
use dzc_protocol::wire::{self, *};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{io, time::Duration};

/// Requests on one connection are correlated and serialized by ownership.
pub struct Client { stream: Option<Stream>, next: u64, pub hello: HelloResult, pub peer: ipc::PeerIdentity }
impl Client {
    pub async fn connect(endpoint: &Endpoint, role: ClientRole, grant: Option<String>) -> io::Result<Self> {
        let mut stream = endpoint.connect(Duration::from_secs(3)).await?;
        let peer = stream.authenticate_after_read()?;
        let hello = HelloParams { protocol_version: wire::VERSION, client_version: env!("CARGO_PKG_VERSION").into(), client_role: role,
            capabilities: vec![], required_capabilities: vec![], grant };
        let request = Request { request_id: "1".into(), method: Method::Hello, params: serde_json::to_value(hello)? };
        let value = exchange(&mut stream, request, "HelloResult", wire::MAX_FRAME_BYTES).await?;
        let hello: HelloResult = serde_json::from_value(value)?;
        if hello.protocol_version.major != wire::VERSION.major { return Err(io::Error::new(io::ErrorKind::InvalidData, "ProtocolMismatch")); }
        Ok(Self { stream: Some(stream), next: 2, hello, peer })
    }
    pub async fn control(endpoint: &Endpoint, role: ClientRole) -> io::Result<Self> {
        // This is an explicit trusted local-client operation, never an automatic
        // consequence of a peer sending a desktop/CLI role in its hello.
        let credentials = ControlCredentials::read(endpoint)?;
        let grant = match role { ClientRole::Cli => credentials.cli, ClientRole::Desktop => credentials.desktop,
            _ => return Err(io::Error::from(io::ErrorKind::PermissionDenied)) };
        Self::connect(endpoint, role, Some(grant)).await
    }
    /// Protocol 1.0 peers may ignore later optional upload fields. Require an
    /// explicit extension version before sending any generated-copy body.
    pub async fn require_generated_copy(&mut self) -> io::Result<()> {
        let capabilities:CapabilityReport=self.call(Method::Capabilities,serde_json::json!({}),"CapabilityReport").await?;
        let feature=&capabilities.platform["desktop_generated_copy"];
        if feature["supported"].as_bool()!=Some(true) || feature["version"].as_u64()!=Some(1) {
            self.stream.take();
            return Err(io::Error::new(io::ErrorKind::Unsupported,"GeneratedCopyCapabilityUnavailable"));
        }
        Ok(())
    }
    pub async fn call<T: DeserializeOwned>(&mut self, method: Method, parameters: impl Serialize, schema: &str) -> io::Result<T> {
        self.next = self.next.checked_add(1).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        let request = Request { request_id: self.next.to_string(), method, params: serde_json::to_value(parameters)? };
        let mut stream = self.stream.take().ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "LocalConnectionClosed"))?;
        let result = exchange(&mut stream, request, schema, self.hello.max_frame_bytes as usize).await;
        if result.as_ref().is_ok() || result.as_ref().is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied) { self.stream = Some(stream); }
        match result.and_then(|value| serde_json::from_value(value).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))) {
            Ok(value) => Ok(value),
            Err(error) => { if error.kind() != io::ErrorKind::PermissionDenied { self.stream.take(); } Err(error) },
        }
    }

    /// One logical Desktop result is reconstructed on this same authenticated
    /// connection; every bounded chunk is checked against its original instance
    /// and epoch. A transport failure poisons the connection rather than reusing
    /// a partly consumed frame for a later request.
    pub async fn read_snapshot<T: DeserializeOwned>(&mut self, snapshot: BodySnapshot, schema: &str) -> io::Result<T> {
        let result=self.read_snapshot_inner(snapshot,schema).await;
        if result.is_err() {self.stream.take();}
        result
    }
    async fn read_snapshot_inner<T: DeserializeOwned>(&mut self, snapshot: BodySnapshot, schema: &str) -> io::Result<T> {
        use base64::Engine;
        if snapshot.total_bytes > 32 * 1024 * 1024 || snapshot.state.service_instance_id != self.hello.state.service_instance_id {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut bytes = Vec::with_capacity(snapshot.total_bytes as usize);
        loop {
            let chunk: SnapshotChunk = tokio::time::timeout_at(deadline, self.call(Method::DesktopSnapshotRead, SnapshotReadParams {
                snapshot_id: snapshot.snapshot_id.clone(), offset: bytes.len() as u32, expected_epoch: snapshot.state.privacy_epoch,
                service_instance_id: snapshot.state.service_instance_id.clone(),
            }, "SnapshotChunk")).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
            let invalid = || io::Error::new(io::ErrorKind::InvalidData, "InvalidSnapshotChunk");
            let decoded = base64::engine::general_purpose::STANDARD.decode(&chunk.data_base64).map_err(|_| invalid())?;
            if chunk.state.service_instance_id != snapshot.state.service_instance_id || chunk.state.privacy_epoch != snapshot.state.privacy_epoch
                || chunk.offset as usize != bytes.len() || chunk.next_offset as usize != bytes.len().saturating_add(decoded.len())
                || chunk.next_offset > snapshot.total_bytes || chunk.finished != (chunk.next_offset == snapshot.total_bytes)
                || (!chunk.finished && decoded.is_empty()) { self.stream.take(); return Err(invalid()); }
            bytes.extend(decoded);
            if chunk.finished { break; }
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        wire::validate(schema, &value).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        serde_json::from_value(value).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
    }
}
async fn exchange(stream: &mut Stream, request: Request, schema: &str, send_limit: usize) -> io::Result<serde_json::Value> {
    let bytes = wire::encode("Request", &request).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "InvalidLocalRequest"))?;
    if send_limit == 0 || send_limit > wire::MAX_FRAME_BYTES || bytes.len() > send_limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "NegotiatedFrameLimitExceeded"));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[test]
    fn native_peer_receive_limit_bounds_post_handshake_request_without_limiting_response() {
        let request = Request { request_id: "3".into(), method: Method::Health, params: serde_json::json!({}) };
        let encoded = wire::encode("Request", &request).unwrap();
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            for limit in [1, encoded.len() - 1, encoded.len(), wire::MAX_FRAME_BYTES] {
                let directory = std::env::temp_dir().join(format!("dzc-peer-cap-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&directory).unwrap();
                #[cfg(target_os = "linux")]
                { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap(); }
                let endpoint = Endpoint::in_directory(&directory).unwrap();
                let mut listener = endpoint.bind().unwrap();
                let expected = encoded.clone();
                let server = tokio::spawn(async move {
                    let mut stream = listener.accept().await.unwrap();
                    let bytes = ipc::read_frame(&mut stream, Duration::from_secs(3)).await.unwrap();
                    stream.authenticate_after_read().unwrap();
                    let hello_request: Request = wire::decode("Request", &bytes).unwrap();
                    let hello = serde_json::to_value(HelloResult { session_id: "synthetic-native-session".into(), protocol_version: VERSION,
                        core_version: env!("CARGO_PKG_VERSION").into(), permissions: vec![], max_frame_bytes: limit as u32,
                        state: RuntimeState { service_instance_id: "synthetic-native-host".into(), history_revision: 0, privacy_epoch: 0, event_sequence: 0 } }).unwrap();
                    let response = wire::encode("Response", &Response { request_id: hello_request.request_id, result: Some(hello), error: None }).unwrap();
                    ipc::write_frame(&mut stream, &response, Duration::from_secs(3)).await.unwrap();
                    if expected.len() > limit {
                        let mut first = [0_u8; 1];
                        match tokio::time::timeout(Duration::from_secs(3), stream.read(&mut first)).await.unwrap() {
                            Ok(0) => (),
                            Err(error) if matches!(error.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset) => (),
                            value => panic!("oversized request reached the peer: {value:?}"),
                        }
                    } else {
                        assert_eq!(ipc::read_frame(&mut stream, Duration::from_secs(3)).await.unwrap(), expected);
                        let response = wire::encode("Response", &Response { request_id: "3".into(), result: Some(serde_json::json!({
                            "service_instance_id":"synthetic-native-host","history_revision":0,"privacy_epoch":0,"event_sequence":0})), error: None }).unwrap();
                        assert!(response.len() > encoded_health_request_length(), "the peer receive cap is not its response size cap");
                        ipc::write_frame(&mut stream, &response, Duration::from_secs(3)).await.unwrap();
                    }
                });
                let mut client = Client::connect(&endpoint, ClientRole::Surface, None).await.unwrap();
                assert_eq!(client.hello.max_frame_bytes as usize, limit);
                let response: io::Result<RuntimeState> = client.call(Method::Health, serde_json::json!({}), "RuntimeState").await;
                if encoded.len() > limit { assert_eq!(response.unwrap_err().kind(), io::ErrorKind::InvalidData); }
                else { response.unwrap(); }
                server.await.unwrap();
                drop(client);
                std::fs::remove_dir_all(directory).unwrap();
            }
        });
    }
    fn encoded_health_request_length() -> usize {
        wire::encode("Request", &Request { request_id: "3".into(), method: Method::Health, params: serde_json::json!({}) }).unwrap().len()
    }
    #[test]
    fn native_legacy_peer_cannot_accept_a_generated_copy_as_an_ordinary_capture() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            for feature in [serde_json::Value::Null,serde_json::json!({"supported":true,"version":0}),
                serde_json::json!({"supported":true,"version":2}),serde_json::json!({"supported":false,"version":1}),
                serde_json::json!({"supported":true,"version":1})] {
                let accepted=feature==serde_json::json!({"supported":true,"version":1});
                let directory=std::env::temp_dir().join(format!("dzc-generated-peer-{}",uuid::Uuid::new_v4()));
                std::fs::create_dir(&directory).unwrap();
                #[cfg(target_os="linux")]
                {use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(&directory,std::fs::Permissions::from_mode(0o700)).unwrap();}
                let endpoint=Endpoint::in_directory(&directory).unwrap();
                let mut listener=endpoint.bind().unwrap();
                let server=tokio::spawn(async move {
                    let mut stream=listener.accept().await.unwrap();
                    let bytes=ipc::read_frame(&mut stream,Duration::from_secs(3)).await.unwrap();
                    stream.authenticate_after_read().unwrap();
                    let request:Request=wire::decode("Request",&bytes).unwrap();
                    let state=RuntimeState{service_instance_id:"synthetic-legacy-host".into(),history_revision:0,privacy_epoch:0,event_sequence:0};
                    let hello=serde_json::to_value(HelloResult{session_id:"synthetic-desktop".into(),protocol_version:VERSION,core_version:env!("CARGO_PKG_VERSION").into(),
                        permissions:vec![Permission::DesktopHistory],state:state.clone(),max_frame_bytes:wire::MAX_FRAME_BYTES as u32}).unwrap();
                    ipc::write_frame(&mut stream,&wire::encode("Response",&Response{request_id:request.request_id,result:Some(hello),error:None}).unwrap(),Duration::from_secs(3)).await.unwrap();
                    let bytes=ipc::read_frame(&mut stream,Duration::from_secs(3)).await.unwrap();
                    let request:Request=wire::decode("Request",&bytes).unwrap();
                    assert_eq!(request.method,Method::Capabilities);
                    let capabilities=serde_json::json!({"methods":["desktop.capture.begin","desktop.capture.write","desktop.capture.finish"],
                        "permissions":["desktop_history"],"platform":if feature.is_null(){serde_json::json!({})}else{serde_json::json!({"desktop_generated_copy":feature})}});
                    ipc::write_frame(&mut stream,&wire::encode("Response",&Response{request_id:request.request_id,result:Some(capabilities),error:None}).unwrap(),Duration::from_secs(3)).await.unwrap();
                    if accepted {
                        let bytes=ipc::read_frame(&mut stream,Duration::from_secs(3)).await.unwrap();
                        let request:Request=wire::decode("Request",&bytes).unwrap();
                        assert_eq!(request.method,Method::DesktopCaptureBegin);
                        assert_eq!(request.params["copy_to_clipboard"],true);
                        let result=serde_json::to_value(CaptureBeginResult{capture_id:Some("1".repeat(32)),state}).unwrap();
                        ipc::write_frame(&mut stream,&wire::encode("Response",&Response{request_id:request.request_id,result:Some(result),error:None}).unwrap(),Duration::from_secs(3)).await.unwrap();
                    } else {
                        let mut first=[0_u8;1];
                        match tokio::time::timeout(Duration::from_secs(3),stream.read(&mut first)).await.unwrap() {
                            Ok(0)=>(),Err(error) if matches!(error.kind(),io::ErrorKind::BrokenPipe|io::ErrorKind::UnexpectedEof|io::ErrorKind::ConnectionReset)=>(),
                            value=>panic!("a generated-copy upload reached an incompatible peer: {value:?}"),
                        }
                    }
                });
                let mut client=Client::connect(&endpoint,ClientRole::Desktop,None).await.unwrap();
                let result=client.require_generated_copy().await;
                if accepted {
                    result.unwrap();
                    let _:CaptureBeginResult=client.call(Method::DesktopCaptureBegin,CaptureBeginParams{service_instance_id:client.hello.state.service_instance_id.clone(),expected_epoch:0,copy_to_clipboard:Some(true)},"CaptureBeginResult").await.unwrap();
                } else {assert_eq!(result.unwrap_err().kind(),io::ErrorKind::Unsupported);}
                drop(client);server.await.unwrap();std::fs::remove_dir_all(directory).unwrap();
            }
        });
    }
}
