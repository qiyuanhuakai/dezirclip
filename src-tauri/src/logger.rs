use chrono::{DateTime, Utc};
use std::fmt::Write as FmtWrite;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// How long the file handle is reused before it is reopened.
///
/// Reopening costs one open/close pair — the very thing this module exists to
/// stop paying per line — so it is paid against a clock instead, roughly forty
/// microseconds per second, which nothing can notice. The reason to reopen at
/// all is that a long-lived handle is a handle to *a file*, not to *that path*:
/// if the log is deleted or replaced while the app runs, writes keep succeeding
/// against the unlinked file and the log quietly stops appearing where anybody
/// looks for it. Reopening on an interval bounds that window without putting a
/// `stat` on every line.
const HANDLE_MAX_AGE: Duration = Duration::from_secs(5);

static LOG: Mutex<Option<LogSink>> = Mutex::new(None);

/// An append-only sink that holds its file open.
///
/// This used to open, write one line and close, for every line. See
/// `costs_one_open_close_per_line` -- an `#[ignore]`d test that measures it, so
/// the numbers quoted below can be re-derived rather than taken on trust. The
/// cost lands wherever the caller is, which for a synchronous `#[tauri::command]`
/// is the main thread.
struct LogSink {
    path: PathBuf,
    handle: Option<File>,
    opened_at: Option<Instant>,
}

impl LogSink {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            handle: None,
            opened_at: None,
        }
    }

    /// Pure decision: is the handle old enough to be worth reopening?
    fn is_stale(opened_at: Option<Instant>, max_age: Duration) -> bool {
        match opened_at {
            Some(at) => at.elapsed() >= max_age,
            // Never opened, or a failed open we want to retry.
            None => true,
        }
    }

    /// The handle to append to.
    ///
    /// `None` while the path cannot be opened, which keeps a line written before
    /// the data directory exists from turning into a permanent loss: the next
    /// line tries again.
    fn handle(&mut self) -> Option<&mut File> {
        if Self::is_stale(self.opened_at, HANDLE_MAX_AGE) {
            // Release the old handle before opening the replacement. Opening
            // first would leave the previous one alive, and a file that was
            // deleted while it was open can sit in a delete-pending state where
            // the reopen fails until the last handle closes — which would cost a
            // line rather than merely delay it. Closing first costs nothing
            // here and does not depend on how a given filesystem behaves.
            drop(self.handle.take());
            self.handle = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
            self.opened_at = self.handle.as_ref().map(|_| Instant::now());
        }
        self.handle.as_mut()
    }

    fn append(&mut self, line: &str) {
        if let Some(handle) = self.handle() {
            let _ = handle.write_all(line.as_bytes());
        }
    }
}

pub fn init(path: PathBuf) {
    if let Ok(mut guard) = LOG.lock() {
        *guard = Some(LogSink::new(path));
    }
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
    // Assembled in full before it reaches the file. `writeln!` against a `File`
    // emits one `WriteFile` syscall per format piece, and per-line syscalls are
    // the whole cost here.
    let now: DateTime<Utc> = SystemTime::now().into();
    let mut line = String::with_capacity(msg.len() + 32);
    let _ = write!(line, "[{}] {}\n", now.format("%Y-%m-%d %H:%M:%S%.3f"), msg);

    if let Ok(mut guard) = LOG.lock() {
        if let Some(sink) = guard.as_mut() {
            sink.append(&line);
        }
    }
    // Deliberately no `flush` and, above all, no `sync_all`. `File` is
    // unbuffered, so `write_all` has already handed the line to the OS page
    // cache and any reader — a tail, another tool, the next run of an analysis —
    // sees it immediately. Verified by reading the file back from a separate
    // handle straight after writing.
    //
    // The numbers, on rustc 1.98.1 / Windows, one 63-byte line. They were taken
    // from `costs_one_open_close_per_line` below, which re-measures them on
    // demand; absolute values move with the machine and with the payload length,
    // so treat the ratio between the arms as the claim and the digits as one
    // reading of it.
    //
    //   formatting only, no file at all            2.24 us
    //   open + append + close per line             167.23 us   <- before
    //   kept open, write_all                         4.26 us   <- now
    //   kept open, write_all + `Write::flush()`      4.26 us
    //   kept open, write_all + `File::sync_all()` 2022.97 us
    //
    // `Write::flush` on a `File` is a no-op on Unix and on Windows — the last
    // but one row is the proof, it costs exactly what doing nothing costs — so
    // the tempting "just add a flush to be safe" is free and pointless.
    // `File::sync_all` is the one that is `FlushFileBuffers` on Windows, and it
    // is three orders of magnitude past what a diagnostic line is worth.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    /// A directory of this test's own, so a leftover from an earlier run can
    /// never decide whether this one passes. Sharing one fixed path across runs
    /// is how these tests went stale once already.
    fn scratch(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let dir = std::env::temp_dir()
            .join("dezirclip-logger-tests")
            .join(format!("{label}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    /// Age a handle out the way the clock eventually would, so a test goes
    /// through the real staleness check rather than poking around it.
    fn age_out(sink: &mut LogSink) {
        sink.opened_at = Some(Instant::now() - HANDLE_MAX_AGE - Duration::from_secs(1));
    }

    #[test]
    fn lines_land_in_the_file_in_order() {
        let dir = scratch("order");
        let path = dir.join("dezirclip.log");
        let mut sink = LogSink::new(path.clone());

        sink.append("[t] first\n");
        sink.append("[t] second\n");

        let written = std::fs::read_to_string(&path).expect("log file exists");
        assert_eq!(written, "[t] first\n[t] second\n");
        drop(sink);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_handle_that_never_opened_stays_willing_to_retry() {
        // A line written before the data directory exists must not turn into a
        // permanent loss, so a failed open is not remembered as a decision.
        // Nothing is poked here on purpose: the retry has to happen on its own.
        let dir = scratch("retry");
        let path = dir.join("nested/out.log");
        let mut sink = LogSink::new(path.clone());

        sink.append("[t] dropped\n");

        std::fs::create_dir_all(path.parent().unwrap()).expect("create the directory late");
        sink.append("[t] recovered\n");

        let written = std::fs::read_to_string(&path).expect("log file exists");
        assert_eq!(written, "[t] recovered\n");
        drop(sink);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_deleted_log_is_picked_up_again() {
        // The reason the handle is not kept forever: a long-lived handle points
        // at a file, not at a path. Delete the file and writes keep succeeding
        // against the unlinked inode, so the log silently stops where the user
        // looks for it — and this log is the only record of what the app did.
        let dir = scratch("deleted");
        let path = dir.join("dezirclip.log");
        let mut sink = LogSink::new(path.clone());

        sink.append("[t] before\n");
        std::fs::remove_file(&path).expect("remove the log underneath the handle");

        age_out(&mut sink);
        sink.append("[t] after\n");

        let written = std::fs::read_to_string(&path).expect("the path is a file again");
        assert_eq!(written, "[t] after\n");
        drop(sink);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_handle_is_reopened_once_it_ages_out_and_kept_before_then() {
        assert!(LogSink::is_stale(None, HANDLE_MAX_AGE));
        assert!(
            !LogSink::is_stale(Some(Instant::now()), HANDLE_MAX_AGE),
            "a fresh handle must not be reopened on the next line"
        );
        assert!(
            LogSink::is_stale(
                Some(Instant::now() - HANDLE_MAX_AGE - Duration::from_secs(1)),
                HANDLE_MAX_AGE
            )
        );
        assert!(
            !LogSink::is_stale(
                Some(Instant::now() - Duration::from_millis(10)),
                HANDLE_MAX_AGE
            )
        );
    }

    #[test]
    fn a_line_written_before_init_is_dropped_rather_than_fatal() {
        // The logger is reachable from paths that run before `init`, and losing
        // a diagnostic is always better than panicking on the way to report it.
        // No other test here installs a sink, so there is nowhere to write.
        write_to_file("[t] written before init");
    }

    /// The cost table quoted in `write_to_file`, measured rather than remembered.
    ///
    /// Ignored by default because it writes to a real file and asserts nothing:
    /// a timing number is not a pass or a fail, and a CI machine's disk is not
    /// the disk anyone should read a ratio off. Run it deliberately:
    ///
    ///     cargo test --bin dezirclip costs_one_open_close_per_line -- --ignored --nocapture
    ///
    /// The numbers quoted in `write_to_file` were taken on rustc 1.98.1, Windows,
    /// one 63-byte line, in this order. Absolute values are machine-specific;
    /// the ratio between the arms is the part worth carrying to another one.
    ///
    /// The "before" arm is the old shape spelled out here -- open, append one
    /// line, drop -- rather than a call into a function that no longer exists.
    /// The "now" arm goes through the real `LogSink`, so a change to the sink
    /// that made it slower would show up in the number instead of silently
    /// invalidating the comment that quotes it.
    #[test]
    #[ignore = "writes to a real file and measures wall clock; run it on purpose"]
    fn costs_one_open_close_per_line() {
        const LINES: usize = 2_000;
        const MSG: &str = "[2026-10-11 03:19:24.417] [idle-destroyer] Showing rebuilt window on its first painted frame.";

        let dir = scratch("bench");
        let path = dir.join("bench.log");

        let started = Instant::now();
        for _ in 0..LINES {
            let now: DateTime<Utc> = SystemTime::now().into();
            let mut line = String::with_capacity(MSG.len() + 32);
            let _ = write!(line, "[{}] {}\n", now.format("%Y-%m-%d %H:%M:%S%.3f"), MSG);
            std::hint::black_box(&line);
        }
        let per_line_format = started.elapsed().as_nanos() as f64 / LINES as f64;

        let started = Instant::now();
        for _ in 0..LINES {
            let now: DateTime<Utc> = SystemTime::now().into();
            let mut line = String::with_capacity(MSG.len() + 32);
            let _ = write!(line, "[{}] {}\n", now.format("%Y-%m-%d %H:%M:%S%.3f"), MSG);
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
                let _ = f.write_all(line.as_bytes());
            }
        }
        let per_line_open_close = started.elapsed().as_nanos() as f64 / LINES as f64;

        let mut sink = LogSink::new(path.clone());
        let started = Instant::now();
        for _ in 0..LINES {
            sink.append(MSG);
            sink.append("\n");
        }
        let per_line_kept_open = started.elapsed().as_nanos() as f64 / LINES as f64;

        let mut sink = LogSink::new(path.clone());
        let started = Instant::now();
        for _ in 0..LINES {
            if let Some(f) = sink.handle() {
                let _ = f.write_all(MSG.as_bytes());
                let _ = f.write_all(b"\n");
                // `Write::flush` on a `File`. Measured because the comment claims
                // it is free, and a claim like that should not be unfalsifiable.
                let _ = f.flush();
            }
        }
        let per_line_flush = started.elapsed().as_nanos() as f64 / LINES as f64;

        let mut sink = LogSink::new(path.clone());
        let started = Instant::now();
        for _ in 0..LINES / 50 {
            if let Some(f) = sink.handle() {
                let _ = f.write_all(MSG.as_bytes());
                let _ = f.sync_all();
            }
        }
        let per_line_sync = started.elapsed().as_nanos() as f64 / (LINES / 50) as f64;

        println!(
            "\nper line, {LINES} lines of {} bytes, dezirclip {} on {}:\n  \
             formatting only, no file at all            {:>9.2} us\n  \
             open + append + close per line             {:>9.2} us   <- before\n  \
             kept open, write_all                       {:>9.2} us   <- now\n  \
             kept open, write_all + Write::flush()      {:>9.2} us\n  \
             kept open, write_all + File::sync_all()    {:>9.2} us\n",
            MSG.len() + 1,
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            per_line_format / 1_000.0,
            per_line_open_close / 1_000.0,
            per_line_kept_open / 1_000.0,
            per_line_flush / 1_000.0,
            per_line_sync / 1_000.0,
        );

        drop(sink);
        let _ = std::fs::remove_dir_all(&dir);
    }
}