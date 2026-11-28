//! Bridges Host copy requests to the Darwin helper's main thread.
use crate::{HostActions, wire::WireErrorCode};
use dzc_core::{api::RuntimeSnapshot, ports::ClipboardPayload};
use dzc_platform::darwin_clipboard::MainThreadClipboard;
use serde_json::{Value, json};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc as reply,
    },
    thread::{self, ThreadId},
};
use tokio::sync::mpsc;

struct WriteText {
    text: String,
    completed: reply::SyncSender<Result<(), WireErrorCode>>,
}

struct ClipboardActions {
    writes: mpsc::Sender<WriteText>,
    main_thread: ThreadId,
    privacy_epoch: Arc<AtomicU64>,
}

/// Owns the pasteboard on the process main thread while Host workers send it
/// text copies. Keep polling during Host shutdown until admitted work drains.
pub struct DarwinClipboardPump {
    clipboard: MainThreadClipboard,
    writes: mpsc::Receiver<WriteText>,
    owned_selection: Option<isize>,
    privacy_epoch: Arc<AtomicU64>,
}

impl DarwinClipboardPump {
    pub fn open() -> io::Result<(Self, Arc<dyn HostActions>)> {
        let clipboard = MainThreadClipboard::open()?;
        let (sender, writes) = mpsc::channel(1);
        let privacy_epoch = Arc::new(AtomicU64::new(0));
        Ok((
            Self {
                clipboard,
                writes,
                owned_selection: None,
                privacy_epoch: privacy_epoch.clone(),
            },
            Arc::new(ClipboardActions {
                writes: sender,
                main_thread: thread::current().id(),
                privacy_epoch,
            }),
        ))
    }

    pub(crate) fn native_clipboard(&self) -> &MainThreadClipboard {
        &self.clipboard
    }

    pub(crate) fn owns_selection(&self, selection: isize) -> bool {
        self.owned_selection == Some(selection)
    }

    pub(crate) fn privacy_epoch(&self) -> u64 {
        self.privacy_epoch.load(Ordering::Acquire)
    }

    pub(crate) fn observe_privacy_epoch(&self, epoch: u64) {
        self.privacy_epoch.fetch_max(epoch, Ordering::AcqRel);
    }

    /// Processes one request. Returns false when the Host has closed the queue.
    pub async fn process_next_write(&mut self) -> bool {
        let Some(write) = self.writes.recv().await else {
            return false;
        };
        let result = self
            .clipboard
            .write_text_owned(&write.text)
            .map(|selection| self.owned_selection = Some(selection))
            .map_err(|_| WireErrorCode::Unavailable);
        let _ = write.completed.send(result);
        true
    }
}

impl HostActions for ClipboardActions {
    fn platform_capabilities(&self) -> Value {
        json!({"native_text_clipboard": true})
    }

    fn can_copy(&self) -> bool {
        true
    }

    fn copy(&self, payload: ClipboardPayload) -> Result<(), WireErrorCode> {
        let ClipboardPayload::Text(text) = payload else {
            return Err(WireErrorCode::UnsupportedCapability);
        };
        if thread::current().id() == self.main_thread {
            return Err(WireErrorCode::Unavailable);
        }
        let (completed, result) = reply::sync_channel(1);
        self.writes
            .try_send(WriteText { text, completed })
            .map_err(|_| WireErrorCode::Unavailable)?;
        result.recv().map_err(|_| WireErrorCode::Unavailable)?
    }

    fn can_paste(&self) -> bool {
        false
    }

    fn paste(&self) -> Result<(), WireErrorCode> {
        Err(WireErrorCode::UnsupportedCapability)
    }

    fn privacy_changed(&self, snapshot: &RuntimeSnapshot) {
        self.privacy_epoch
            .fetch_max(snapshot.privacy_epoch, Ordering::AcqRel);
    }

    fn stop_helpers(&self) -> Result<(), WireErrorCode> {
        Ok(())
    }

    fn revoke_private_exports(&self) -> Result<(), WireErrorCode> {
        Ok(())
    }
}
