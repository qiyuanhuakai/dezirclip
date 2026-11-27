use rusqlite::{Connection, OpenFlags};
use std::{fs::File, path::Path, time::Duration};

/// Read the legacy GPU preference without initializing storage. The adapter
/// must hold SQLite's exclusive main-file lock for the lifetime of its handle.
pub fn probe_startup_disable_webview_gpu(
    path: &Path,
    lock_database: impl FnOnce(&Path) -> Option<File>,
) -> bool {
    stored_gpu_preference(path, lock_database)
        .is_some_and(|value| matches!(value.as_str(), "true" | "1" | "TRUE" | "True"))
}

fn stored_gpu_preference(path: &Path, lock_database: impl FnOnce(&Path) -> Option<File>) -> Option<String> {
    let path = path.canonicalize().ok()?;
    if !path.is_file() {
        return None;
    }
    let _guard = lock_database(&path)?;
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut related = path.as_os_str().to_owned();
        related.push(suffix);
        if Path::new(&related).try_exists().ok()? {
            return None;
        }
    }
    // The exclusive lock prevents a SQLite peer from creating a journal while
    // immutable mode reads the checkpointed main file without opening sidecars.
    let encoded_path = urlencoding::encode_binary(path.as_os_str().as_encoded_bytes());
    let uri = format!("file:{encoded_path}?immutable=1");
    let connection = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI).ok()?;
    connection.busy_timeout(Duration::ZERO).ok()?;
    connection.query_row(
        "SELECT value FROM settings WHERE key = 'app.disable_webview_gpu' LIMIT 1",
        [],
        |row| row.get(0),
    ).ok()
}
