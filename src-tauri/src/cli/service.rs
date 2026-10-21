//! Explicit service and privacy management; ordinary commands never auto-start it.
use clap::{Args, Subcommand};
use dzc_host::{Client, controller, wire::*};
use std::path::PathBuf;

#[derive(Args)]
pub struct ServiceArgs { #[command(subcommand)] command: ServiceCommand }
#[derive(Subcommand)]
enum ServiceCommand {
    /// Start the optional service, acknowledging any existing embedded capture handoff.
    Start { #[arg(long)] no_capture: bool, #[arg(long)] json: bool },
    /// Stop and drain the current Host; a failed stop remains a failure.
    Stop,
    /// Query the current Host without starting one.
    Status { #[arg(long)] json: bool },
    /// Explicitly grant a Surface bounded access. The token is printed once.
    GrantSurface { #[arg(long)] copy: bool, #[arg(long)] paste: bool,
        #[arg(long,default_value_t=3600,value_parser=clap::value_parser!(u64).range(1..=86400))] seconds: u64 },
    /// Delegate bounded ordinary text reads to `mcp --host`; never mutation or sensitive access.
    GrantMcp { #[arg(long,default_value_t=3600,value_parser=clap::value_parser!(u64).range(1..=86400))] seconds:u64 },
    /// Revoke a previously issued delegated grant, including live connections.
    Revoke { token: String },
}
#[derive(Args)]
pub struct PrivacyArgs { #[command(subcommand)] command: PrivacyCommand }
#[derive(Subcommand)]
enum PrivacyCommand {
    On { #[arg(long,value_parser=clap::value_parser!(u64).range(1..=86400))] seconds: Option<u64>, #[arg(long)] json: bool },
    Off { #[arg(long)] json: bool },
    Status { #[arg(long)] json: bool },
}
fn runtime() -> Result<tokio::runtime::Runtime,String> { tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "LocalHostRuntimeUnavailable".into()) }
fn database() -> Result<PathBuf,String> { dzc_host::data::database_path() }
pub fn run(args: ServiceArgs) -> Result<(),String> {
    let database = database()?;
    runtime()?.block_on(async {
        match args.command {
            ServiceCommand::Start { no_capture, json } => {
                let status = controller::start(&database, &controller::sibling_executable().map_err(|e|e.to_string())?, no_capture).await.map_err(|e|e.to_string())?;
                status_output(status,json)
            }
            ServiceCommand::Status { json } => status_output(controller::status(&database).await.map_err(|e|e.to_string())?,json),
            ServiceCommand::Stop => controller::stop(&database).await.map_err(|e|e.to_string()),
            ServiceCommand::GrantSurface { copy,paste,seconds } => {
                let mut client = Client::control(&controller::endpoint(&database).map_err(|e|e.to_string())?,ClientRole::Cli).await.map_err(|e|e.to_string())?;
                let mut permissions = vec!["history_read", "surface_control"];
                if copy || paste { permissions.push("clipboard_copy"); } if paste { permissions.push("clipboard_paste"); }
                let grant: GrantResult = client.call(Method::ServiceGrant,serde_json::json!({"role":"surface","permissions":permissions,"lifetime_ms":seconds*1000}),"GrantResult").await.map_err(|e|e.to_string())?;
                println!("{}",serde_json::to_string(&grant).map_err(|e|e.to_string())?); Ok(())
            }
            ServiceCommand::Revoke { token } => {
                let mut client = Client::control(&controller::endpoint(&database).map_err(|e|e.to_string())?,ClientRole::Cli).await.map_err(|e|e.to_string())?;
                let _: OperationResult = client.call(Method::ServiceRevoke,serde_json::json!({"grant":token}),"OperationResult").await.map_err(|e|e.to_string())?; Ok(())
            }
            ServiceCommand::GrantMcp {seconds}=>{
                let mut client=Client::control(&controller::endpoint(&database).map_err(|error|error.to_string())?,ClientRole::Cli).await.map_err(|error|error.to_string())?;
                let grant:GrantResult=client.call(Method::ServiceGrant,serde_json::json!({"role":"mcp","permissions":["history_read","history_body"],"lifetime_ms":seconds*1000}),"GrantResult").await.map_err(|error|error.to_string())?;
                println!("{}",serde_json::to_string(&grant).map_err(|error|error.to_string())?);Ok(())
            }
        }
    })
}
fn status_output(status: ServiceStatus,json:bool) -> Result<(),String> {
    if json { println!("{}",serde_json::to_string(&status).map_err(|e|e.to_string())?); }
    else { println!("Host {:?}; capture owner: {}; privacy: {}",status.mode,status.capture_owner,status.privacy.active); }
    Ok(())
}
pub fn privacy(args: PrivacyArgs) -> Result<(),String> {
    let database = database()?;
    let (command,json) = match args.command {
        PrivacyCommand::On { seconds,json } => (Some(dzc_core::api::PrivacySessionCommand::On { duration_ms:seconds.map(|s|s*1000) }),json),
        PrivacyCommand::Off { json } => (Some(dzc_core::api::PrivacySessionCommand::Off),json),
        PrivacyCommand::Status { json } => (None,json),
    };
    runtime()?.block_on(async {
        let endpoint = controller::endpoint(&database).map_err(|e|e.to_string())?;
        match Client::control(&endpoint,ClientRole::Cli).await {
            Ok(mut client) => {
                let state: PrivacyState = match command {
                    Some(dzc_core::api::PrivacySessionCommand::On { duration_ms }) => client.call(Method::PrivacySet,PrivacySetParams {active:true,duration_ms},"PrivacyState").await,
                    Some(dzc_core::api::PrivacySessionCommand::Off) => client.call(Method::PrivacySet,serde_json::json!({"active":false}),"PrivacyState").await,
                    None => client.call(Method::PrivacyGet,serde_json::json!({}),"PrivacyState").await,
                }.map_err(|e|e.to_string())?;
                if json { println!("{}",serde_json::to_string(&state).map_err(|e|e.to_string())?); } else { println!("Privacy session: {}",if state.active {"on"} else {"off"}); }
                Ok(())
            }
            Err(error) if matches!(error.kind(),std::io::ErrorKind::NotFound|std::io::ErrorKind::ConnectionRefused) => {
                if Client::connect(&endpoint,ClientRole::Cli,None).await.is_ok() { return Err("HostControlPermissionUnavailable".into()); }
                let history = dzc_core::api::HistoryService::open(&database).map_err(|e|e.to_string())?;
                let now = chrono::Utc::now().timestamp_millis();
                let snapshot = if let Some(command)=command {history.privacy().set_session(command,now)} else {history.runtime_snapshot(now)}.map_err(|e|e.to_string())?;
                let state = PrivacyState { active:snapshot.session.active,expires_at_ms:snapshot.session.expires_at_ms,privacy_epoch:snapshot.privacy_epoch };
                if json { println!("{}",serde_json::to_string(&state).map_err(|e|e.to_string())?); } else { println!("Privacy session: {}",if state.active {"on"} else {"off"}); } Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
    })
}
