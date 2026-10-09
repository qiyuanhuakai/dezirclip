use chrono::{DateTime, Utc};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(path: PathBuf) {
    let mut guard = LOG_PATH.lock().unwrap();
    *guard = Some(path);
}

pub fn log(msg: &str) {
    write_to_file(msg);
    // Also print to console for development
    println!("{}", msg);
}

/// Diagnostics, for callers whose stdout is a data channel.
///
/// `dzc list --json` parses stdout, and `--ids` pipes it into another tool. A
/// warning about a key that could not be read is exactly the moment a user is
/// most likely to be running the CLI, and on stdout it corrupts both: the JSON
/// no longer parses and the IDs cannot be read. It still belongs in the log
/// file either way, which `write_to_file` above already covers.
pub fn log_diagnostic(msg: &str) {
    write_to_file(msg);
    eprintln!("{}", msg);
}

fn write_to_file(msg: &str) {
    if let Ok(guard) = LOG_PATH.lock() {
        if let Some(path) = &*guard {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                let now: DateTime<Utc> = SystemTime::now().into();
                let timestamp = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                let _ = writeln!(f, "[{}] {}", timestamp, msg);
            }
        }
    }
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::logger::log(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::logger::log_diagnostic(&format!("[ERROR] {}", format!($($arg)*)))
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::logger::log_diagnostic(&format!("[WARN] {}", format!($($arg)*)))
    };
}
