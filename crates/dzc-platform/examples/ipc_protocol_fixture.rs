//! Synthetic cross-language transport fixture: no database, clipboard or GUI.
use dzc_platform::ipc::{read_frame, write_frame, Endpoint};
use dzc_protocol::wire::{self, HelloResult, Method, Permission, ProtocolVersion, Request, Response, RuntimeState};
use std::{io::{self, Write}, path::Path, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--identity-client") {
        return identity_client();
    }
    let directory = std::env::args_os().nth(1).ok_or("private fixture directory required")?;
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let endpoint = Endpoint::in_directory(Path::new(&directory))?;
        let mut listener = endpoint.bind()?;
        println!("{}", endpoint.address().display());
        io::stdout().flush()?;
        let mut stream = tokio::time::timeout(Duration::from_secs(20), listener.accept()).await??;
        for index in 0..3 {
            let bytes = read_frame(&mut stream, Duration::from_secs(5)).await?;
            let _peer = stream.authenticate_after_read()?;
            let request: Request = wire::decode("Request", &bytes).map_err(|_| "invalid fixture request")?;
            let result = match (index, request.method) {
                (0, Method::Hello) => serde_json::to_value(HelloResult {
                    session_id: "synthetic-session".into(), protocol_version: ProtocolVersion { major: 1, minor: 0 },
                    core_version: "fixture".into(), permissions: vec![Permission::HistoryRead],
                    state: RuntimeState { service_instance_id: "synthetic-instance".into(), history_revision: 3, privacy_epoch: 2, event_sequence: 0 },
                    max_frame_bytes: wire::MAX_FRAME_BYTES as u32,
                })?,
                (1, Method::Health) => serde_json::json!({}),
                (2, Method::SurfaceHistoryList) => serde_json::json!({"entries":[]}),
                _ => return Err("unexpected fixture method".into()),
            };
            let response = Response::success(request.request_id, result);
            let bytes = wire::encode("Response", &response).map_err(|_| "invalid fixture response")?;
            write_frame(&mut stream, &bytes, Duration::from_secs(5)).await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

fn identity_client() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os().nth(2).ok_or("private fixture directory required")?;
    let expected_pid: u32 = std::env::args().nth(3).ok_or("server PID required")?.parse()?;
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let endpoint = Endpoint::in_directory(Path::new(&directory))?;
        let mut stream = endpoint.connect(Duration::from_secs(2)).await?;
        if stream.authenticate_after_read()?.pid() != Some(expected_pid) { return Err("wrong authenticated server PID".into()); }
        for (index, (method, params)) in [
            (Method::Hello, serde_json::json!({"protocol_version":{"major":1,"minor":0},"client_version":"fixture","client_role":"cli","capabilities":[],"required_capabilities":[]})),
            (Method::Health, serde_json::json!({})),
            (Method::SurfaceHistoryList, serde_json::json!({"limit":20,"offset":0})),
        ].into_iter().enumerate() {
            let request = Request { request_id: index.to_string(), method, params };
            let bytes = wire::encode("Request", &request).map_err(|_| "invalid fixture request")?;
            write_frame(&mut stream, &bytes, Duration::from_secs(2)).await?;
            let bytes = read_frame(&mut stream, Duration::from_secs(2)).await?;
            let _: Response = wire::decode("Response", &bytes).map_err(|_| "invalid fixture response")?;
        }
        println!("native server PID verified: {expected_pid}");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}
