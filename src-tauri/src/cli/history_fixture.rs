use dzc_core::api::HistoryService;
use rusqlite::Connection;
use tempfile::TempDir;

/// Creates an older on-disk history so tests can seed records the public
/// writer would reject, then opens it through the same path as the CLI.
pub(super) struct LegacyHistory(TempDir);

impl LegacyHistory {
    pub(super) fn new(seed: impl FnOnce(&Connection)) -> Self {
        let fixture = Self(tempfile::tempdir().unwrap());
        let connection = Connection::open(fixture.0.path().join("history.db")).unwrap();
        connection.execute_batch("
            CREATE TABLE clipboard_history (
                id INTEGER PRIMARY KEY,
                content_type TEXT NOT NULL,
                content TEXT NOT NULL,
                source_app TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                preview TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '[]',
                html_content TEXT,
                source_app_path TEXT,
                ocr_text TEXT
            );
        ").unwrap();
        seed(&connection);
        fixture
    }

    pub(super) fn open(&self) -> HistoryService {
        HistoryService::open(&self.0.path().join("history.db")).unwrap()
    }

    pub(super) fn read_only(&self) -> Connection {
        Connection::open_with_flags(
            self.0.path().join("history.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).unwrap()
    }
}
