pub mod database;
pub mod domain;

// Application diagnostics remain outside Core; native adapters use an injected sink.
pub mod logger;

#[path = "cli_infrastructure.rs"]
pub mod infrastructure;

#[path = "cli_services.rs"]
pub mod services;

pub mod cli;

pub use dzc_core::api;
pub use dzc_core::ports;
pub use dzc_core::protocol;
