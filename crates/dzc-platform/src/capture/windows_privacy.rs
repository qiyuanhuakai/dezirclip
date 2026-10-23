//! Windows history opt-out metadata, inspected before reading any body representation.
use dzc_core::capture::{CaptureFailure, CaptureStage, PrivacyHintAvailability, PrivacyPreflight};
use dzc_core::ports::PrivacyHint;
use windows::core::w;
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

fn failure() -> CaptureFailure {
    CaptureFailure {
        stage: CaptureStage::Read,
        code: "PrivacyHintReadUnavailable".into(),
    }
}

pub fn selection_id() -> Option<u64> {
    let sequence = unsafe { GetClipboardSequenceNumber() };
    (sequence != 0).then_some(u64::from(sequence))
}

fn excludes_history(excluded_format_present: bool, include_history: Option<u32>) -> bool {
    excluded_format_present || include_history == Some(0)
}

/// Reads at most one DWORD. Cloud-upload-only preferences do not imply local history opt-out.
pub fn preflight() -> Result<PrivacyPreflight, CaptureFailure> {
    preflight_for(None)
}

pub(crate) fn preflight_for(expected: Option<u64>) -> Result<PrivacyPreflight, CaptureFailure> {
    let sequence = expected.or_else(selection_id).filter(|value| *value != 0).ok_or_else(failure)?;
    unsafe {
        let excluded = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let include = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if excluded == 0 || include == 0 {
            return Err(failure());
        }
        struct OpenGuard;
        impl Drop for OpenGuard {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }
        let _guard = super::acquisition::acquire_same_selection(sequence, selection_id,
            || OpenClipboard(None).ok().map(|_| OpenGuard), std::thread::sleep).map_err(|_| failure())?;
        let excluded_present = IsClipboardFormatAvailable(excluded).is_ok();
        let include_history = if !excluded_present && IsClipboardFormatAvailable(include).is_ok() {
            let handle = GetClipboardData(include).map_err(|_| failure())?;
            let global = HGLOBAL(handle.0);
            if !(std::mem::size_of::<u32>()..=64).contains(&GlobalSize(global)) {
                return Err(failure());
            }
            let pointer = GlobalLock(global);
            if pointer.is_null() {
                return Err(failure());
            }
            let value = std::ptr::read_unaligned(pointer.cast::<u32>());
            let _ = GlobalUnlock(global);
            if value > 1 {
                return Err(failure());
            }
            Some(value)
        } else {
            None
        };
        if selection_id() != Some(sequence) {
            return Err(failure());
        }
        Ok(PrivacyPreflight {
            hints: if excludes_history(excluded_present, include_history) {
                vec![PrivacyHint::ExcludeFromHistory]
            } else {
                Vec::new()
            },
            availability: PrivacyHintAvailability::Checked,
            selection_id: Some(sequence),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn microsoft_history_opt_out_metadata_has_exact_semantics() {
        assert!(excludes_history(true, None));
        assert!(excludes_history(true, Some(1)));
        assert!(excludes_history(false, Some(0)));
        assert!(!excludes_history(false, Some(1)));
        assert!(!excludes_history(false, None));
    }
}
