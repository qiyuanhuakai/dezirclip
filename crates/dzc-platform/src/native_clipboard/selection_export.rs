//! Scoped file exports close when their actual native selection worker exits.
use crate::private_files::BoundExport;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct SelectionExport(Arc<Mutex<Option<BoundExport>>>);
impl SelectionExport {
    pub(crate) fn new(export: Option<BoundExport>) -> Self {
        Self(Arc::new(Mutex::new(export)))
    }
    pub(crate) fn close(&self) -> Result<(), String> {
        let mut slot = self.0.lock().map_err(|_| "PrivateExportRevocationUnconfirmed")?;
        if let Some(export) = slot.as_mut() {
            export.close()?;
        }
        *slot = None;
        Ok(())
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.0.lock().is_ok_and(|slot| slot.is_none())
    }
}
