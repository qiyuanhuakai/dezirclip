use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::database::DbState;

#[derive(Clone, Copy, Debug)]
pub enum EncryptionAction {
    Encrypt,
    Decrypt,
}

#[derive(Clone, Copy, Debug)]
pub struct EncryptionJob {
    pub id: i64,
    pub action: EncryptionAction,
}

#[derive(Clone)]
pub struct EncryptionQueue {
    sender: Sender<EncryptionJob>,
}

impl EncryptionQueue {
    pub fn enqueue(&self, job: EncryptionJob) {
        let _ = self.sender.send(job);
    }
}

pub fn init_encryption_queue(app_handle: AppHandle) -> EncryptionQueue {
    let (tx, rx) = mpsc::channel::<EncryptionJob>();
    thread::spawn(move || worker(app_handle, rx));
    EncryptionQueue { sender: tx }
}

/// How long to wait after a *drain* before asking for more work.
///
/// This used to be a 30 ms sleep after every individual job, inside a loop that
/// had already pulled the whole channel into a batch and released the database
/// lock. Nothing was being waited for: the next jobs were already in memory, the
/// lock was already free, and the thread read no shared state between iterations.
///
/// On the machine this was measured on, DPAPI alone costs 1.3-3.2 ms per entry,
/// which puts the sleep at 90-96% of a job. Treat that as an upper bound and a
/// local estimate, not a portable ratio: a real job also runs a SELECT, an
/// UPDATE and a text hash under the lock, it encrypts 2 values or 3 depending on
/// whether the entry carries `html_content`. Portable builds reject sensitive
/// persistence before DPAPI, so that timing does not describe portable jobs.
/// See `dpapi_per_value_cost` in `crate::infrastructure::encryption`.
///
/// What the pacing is actually for is to leave the database connection free for
/// ordinary capture work, so it is kept -- but it now runs once per drained
/// batch rather than once per job. One job still yields the same single pause,
/// so single-job latency is unchanged; a burst of N jobs that previously paid
/// N * 30 ms now pays one.
///
/// What that costs: the old sleep also spaced this queue's lock acquisitions at
/// least 30 ms apart. Now the N jobs of a batch take the connection back to
/// back. The queue has a single enqueue site (`update_entry_tags`, when an entry
/// crosses the sensitive-tag boundary), but that bounds how often jobs arrive,
/// not how many can pile up in one drain -- a run of rapid tag edits is one
/// batch. Whether that measurably hurts the capture thread's tail latency has
/// not been tested here.
const BATCH_PAUSE: Duration = Duration::from_millis(30);

/// Run every job in a drained batch, pausing once at the end, and report how
/// many pauses that took.
///
/// Extracted so the pacing is observable in a test: `run_job` is the per-entry
/// work (encrypt or decrypt) and `pause` is the yield, both injected. The pause
/// count is counted here rather than read back from `pause`, so it stays a fact
/// about this loop even when `pause` is a stub.
fn drain_batch<R, P>(job_count: usize, mut run_job: R, mut pause: P) -> usize
where
    R: FnMut(usize) -> Result<(), String>,
    P: FnMut(Duration),
{
    let mut pauses = 0;
    for index in 0..job_count {
        if let Err(err) = run_job(index) {
            eprintln!("encryption queue job failed: {}", err);
        }
        if index + 1 == job_count {
            pause(BATCH_PAUSE);
            pauses += 1;
        }
    }
    pauses
}

fn worker(app_handle: AppHandle, rx: Receiver<EncryptionJob>) {
    while let Ok(job) = rx.recv() {
        let mut jobs = vec![job];
        while let Ok(next) = rx.try_recv() {
            jobs.push(next);
        }

        // The connection is free between jobs, so a single yield after the batch
        // is what leaves it available to the capture path.
        drain_batch(
            jobs.len(),
            |index| {
                let job = jobs[index];
                let db_state = app_handle.state::<DbState>();
                // A poisoned lock is not contention, so there is nothing to wait
                // out: the job cannot succeed either way. Contention, which
                // `lock` blocks on, is already handled by blocking here.
                let conn = match db_state.conn.lock() {
                    Ok(c) => c,
                    Err(_) => return Ok(()),
                };

                let result = match job.action {
                    EncryptionAction::Encrypt => {
                        db_state.repo.encrypt_entry_with_conn(&conn, job.id)
                    }
                    EncryptionAction::Decrypt => {
                        db_state.repo.decrypt_entry_with_conn(&conn, job.id)
                    }
                };
                drop(conn);
                result.map_err(|err| format!("id={}: {}", job.id, err))
            },
            |d| {
                thread::sleep(d);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{drain_batch, BATCH_PAUSE};
    use std::time::Duration;

    /// Records every pause the batch takes, with the duration it was handed, by
    /// standing in for `thread::sleep`. Returns the same tuple the assertions
    /// read, so a stub that silently did nothing would show up as zero pauses.
    struct PauseLog {
        taken: Vec<Duration>,
    }

    impl PauseLog {
        fn new() -> Self {
            PauseLog { taken: Vec::new() }
        }

        /// The stub `sleep` itself: it appends, rather than reporting a number
        /// the caller supplied back to itself.
        fn sleep(&mut self, d: Duration) {
            self.taken.push(d);
        }
    }

    /// A drained batch pauses once, whatever it holds.
    ///
    /// The point of the change is the *count*, not the duration: N jobs used to
    /// sleep N times. Counting the pauses is what makes that observable without
    /// depending on wall-clock timing.
    #[test]
    fn a_drained_batch_pauses_once() {
        for batch in [1usize, 2, 5, 20] {
            let mut log = PauseLog::new();
            let pauses = drain_batch(
                batch,
                |_| Ok(()),
                |d| log.sleep(d),
            );
            assert_eq!(
                pauses, 1,
                "a batch of {batch} jobs must pause once, not {batch} times"
            );
            assert_eq!(
                log.taken.len(),
                1,
                "the sleep must actually run {batch} times' worth of one pause, \
                 not be reported without happening"
            );
        }
    }

    /// The one pause is the same 30 ms the old per-job code slept for, and it is
    /// handed to `sleep` rather than being re-derived from the constant.
    #[test]
    fn the_pause_keeps_its_duration() {
        assert_eq!(BATCH_PAUSE.as_millis(), 30);
        let mut log = PauseLog::new();
        drain_batch(4, |_| Ok(()), |d| log.sleep(d));
        assert_eq!(
            log.taken,
            vec![BATCH_PAUSE],
            "the batch must yield once, for exactly the old duration"
        );
    }

    /// Every job still runs, whatever the batch size -- the pause moved, the work
    /// did not.
    #[test]
    fn every_job_in_the_batch_is_run() {
        for batch in [1usize, 3, 7] {
            let mut ran = Vec::new();
            let mut log = PauseLog::new();
            let pauses = drain_batch(
                batch,
                |id| {
                    ran.push(id);
                    Ok(())
                },
                |d| log.sleep(d),
            );
            assert_eq!(ran, (0..batch).collect::<Vec<_>>(), "no job may be skipped");
            assert_eq!(pauses, 1);
            assert_eq!(log.taken.len(), 1);
        }
    }

    /// The yield happens *after* the work, not before it. If it were hoisted to
    /// the top of the loop the first job would stall while the connection sits
    /// idle, which is the opposite of what the pacing is for.
    #[test]
    fn the_pause_comes_after_the_work() {
        let order = std::cell::RefCell::new(Vec::new());
        let mut log = PauseLog::new();
        drain_batch(
            3,
            |id| {
                order.borrow_mut().push(format!("job{id}"));
                Ok(())
            },
            |d| {
                order.borrow_mut().push("pause".to_string());
                log.sleep(d);
            },
        );
        assert_eq!(*order.borrow(), vec!["job0", "job1", "job2", "pause"]);
    }

    /// A failing job must not stop the rest of the batch.
    #[test]
    fn one_failing_job_does_not_abort_the_batch() {
        let mut ran = Vec::new();
        let mut log = PauseLog::new();
        let pauses = drain_batch(
            4,
            |id| {
                ran.push(id);
                if id == 1 {
                    Err("boom".to_string())
                } else {
                    Ok(())
                }
            },
            |d| log.sleep(d),
        );
        assert_eq!(ran, vec![0, 1, 2, 3], "the batch must run to completion");
        assert_eq!(pauses, 1);
        assert_eq!(log.taken.len(), 1);
    }
}
