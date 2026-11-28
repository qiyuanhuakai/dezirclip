//! Bridges Host copy requests to the Darwin helper's main thread.
use crate::{HostActions, wire::WireErrorCode};
use dzc_core::ports::ClipboardPayload;
use dzc_platform::darwin_clipboard::MainThreadClipboard;
use serde_json::{Value, json};
use std::{
    io,
    sync::{Arc, mpsc as reply},
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
}

/// Owns the pasteboard on the process main thread while Host workers send it
/// text copies. Keep polling during Host shutdown until admitted work drains.
pub struct DarwinClipboardPump {
    clipboard: MainThreadClipboard,
    writes: mpsc::Receiver<WriteText>,
}

impl DarwinClipboardPump {
    pub fn open() -> io::Result<(Self, Arc<dyn HostActions>)> {
        let clipboard = MainThreadClipboard::open()?;
        let (sender, writes) = mpsc::channel(1);
        Ok((
            Self { clipboard, writes },
            Arc::new(ClipboardActions {
                writes: sender,
                main_thread: thread::current().id(),
            }),
        ))
    }

    /// Processes one request. Returns false when the Host has closed the queue.
    pub async fn process_next_write(&mut self) -> bool {
        let Some(write) = self.writes.recv().await else {
            return false;
        };
        let result = self
            .clipboard
            .write_text(&write.text)
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

    fn stop_helpers(&self) -> Result<(), WireErrorCode> {
        Ok(())
    }

    fn revoke_private_exports(&self) -> Result<(), WireErrorCode> {
        Ok(())
    }
}
