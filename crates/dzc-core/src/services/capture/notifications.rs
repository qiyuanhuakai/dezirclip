use super::{CaptureReadMetadata, CaptureSkip};

#[derive(Debug, Clone, Copy, Default)]
pub struct OwnClipboardCopy {
    pub primary: u64,
    pub alternative: u64,
    pub age_seconds: u64,
}

#[derive(Default)]
pub struct CaptureNotificationState {
    sequence: Option<u32>,
    fingerprint: Option<u64>,
    timestamp_ms: u64,
    image_hash: Option<u64>,
    file_hash: Option<u64>,
    persistence_mode: Option<bool>,
}

impl CaptureNotificationState {
    pub fn at_sequence(sequence: u32) -> Self {
        Self {
            sequence: Some(sequence),
            ..Self::default()
        }
    }

    pub fn accept_sequence(&mut self, sequence: u32) -> bool {
        if self.sequence == Some(sequence) {
            return false;
        }
        self.sequence = Some(sequence);
        true
    }

    /// A new destination must receive its first capture even if the payload is unchanged.
    pub fn set_persistence_mode(&mut self, persistent: bool) {
        if self.persistence_mode.replace(persistent) != Some(persistent) {
            self.fingerprint = None;
            self.timestamp_ms = 0;
            self.image_hash = None;
            self.file_hash = None;
        }
    }

    pub fn inspect(
        &mut self,
        metadata: CaptureReadMetadata,
        timestamp_ms: u64,
        own: OwnClipboardCopy,
        repeated_file_still_exists: bool,
    ) -> Result<(), CaptureSkip> {
        if !metadata.image {
            self.image_hash = None;
        }
        if !metadata.files {
            self.file_hash = None;
        }
        let duplicate = metadata.fingerprint.is_some()
            && metadata.fingerprint == self.fingerprint
            && timestamp_ms.saturating_sub(self.timestamp_ms) < 500
            && (!metadata.files || repeated_file_still_exists);
        self.fingerprint = metadata.fingerprint;
        self.timestamp_ms = timestamp_ms;
        if duplicate {
            return Err(CaptureSkip::DuplicateNotification);
        }

        let own_copy = own.primary != 0
            && own.age_seconds < 10
            && metadata
                .echo_hash
                .is_some_and(|hash| hash == own.primary || hash == own.alternative);
        if own_copy {
            if metadata.image && !metadata.files {
                self.image_hash = metadata.echo_hash;
            }
            if metadata.files {
                self.file_hash = metadata.fingerprint;
                self.image_hash = None;
            }
            return Err(CaptureSkip::OwnClipboard);
        }
        if metadata.files {
            let repeated = metadata.fingerprint.is_some() && self.file_hash == metadata.fingerprint;
            self.file_hash = metadata.fingerprint;
            self.image_hash = None;
            if repeated && repeated_file_still_exists {
                return Err(CaptureSkip::DuplicateNotification);
            }
        } else if metadata.image {
            let repeated = metadata.echo_hash.is_some() && self.image_hash == metadata.echo_hash;
            self.image_hash = metadata.echo_hash;
            self.file_hash = None;
            if repeated {
                return Err(CaptureSkip::DuplicateNotification);
            }
        } else {
            self.file_hash = None;
        }
        Ok(())
    }

    pub fn repeats_file(&self, metadata: CaptureReadMetadata) -> bool {
        metadata.files && metadata.fingerprint.is_some() && self.file_hash == metadata.fingerprint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(hash: u64) -> CaptureReadMetadata {
        CaptureReadMetadata {
            fingerprint: Some(hash),
            echo_hash: Some(hash),
            image: false,
            files: false,
            image_content_hash: None,
        }
    }

    #[test]
    fn sequence_and_time_window_dedup_do_not_suppress_new_payloads() {
        let mut state = CaptureNotificationState::at_sequence(10);
        assert!(!state.accept_sequence(10));
        assert!(state.accept_sequence(11));
        assert!(state
            .inspect(metadata(1), 1000, OwnClipboardCopy::default(), false)
            .is_ok());
        assert_eq!(
            state.inspect(metadata(1), 1100, OwnClipboardCopy::default(), false),
            Err(CaptureSkip::DuplicateNotification)
        );
        assert!(state
            .inspect(metadata(2), 1101, OwnClipboardCopy::default(), false)
            .is_ok());
        assert!(state
            .inspect(metadata(2), 1601, OwnClipboardCopy::default(), false)
            .is_ok());
    }

    #[test]
    fn own_copy_uses_both_fingerprints_with_a_bounded_age() {
        for hash in [7, 8] {
            let mut state = CaptureNotificationState::default();
            let own = OwnClipboardCopy {
                primary: 7,
                alternative: 8,
                age_seconds: 9,
            };
            assert_eq!(
                state.inspect(metadata(hash), 1, own, false),
                Err(CaptureSkip::OwnClipboard)
            );
            assert!(state
                .inspect(
                    metadata(hash),
                    1000,
                    OwnClipboardCopy {
                        age_seconds: 10,
                        ..own
                    },
                    false
                )
                .is_ok());
        }
    }

    #[test]
    fn identical_images_remain_suppressed_but_deleted_file_history_can_return() {
        let mut state = CaptureNotificationState::default();
        let image = CaptureReadMetadata {
            image: true,
            ..metadata(1)
        };
        assert!(state
            .inspect(image, 1000, OwnClipboardCopy::default(), false)
            .is_ok());
        assert_eq!(
            state.inspect(image, 2000, OwnClipboardCopy::default(), false),
            Err(CaptureSkip::DuplicateNotification)
        );
        let file = CaptureReadMetadata {
            files: true,
            ..metadata(2)
        };
        assert!(state
            .inspect(file, 3000, OwnClipboardCopy::default(), true)
            .is_ok());
        assert!(state.repeats_file(file));
        assert_eq!(
            state.inspect(file, 4000, OwnClipboardCopy::default(), true),
            Err(CaptureSkip::DuplicateNotification)
        );
        assert!(state
            .inspect(file, 5000, OwnClipboardCopy::default(), false)
            .is_ok());
    }

    #[test]
    fn decoded_file_images_obey_history_existence_and_format_transitions() {
        let image = CaptureReadMetadata {
            image: true,
            files: true,
            image_content_hash: Some(10),
            ..metadata(1)
        };
        let other = CaptureReadMetadata {
            image: true,
            files: true,
            image_content_hash: Some(20),
            ..metadata(2)
        };
        let mut state = CaptureNotificationState::default();
        assert!(state
            .inspect(image, 1000, OwnClipboardCopy::default(), false)
            .is_ok());
        assert!(state.repeats_file(image));
        assert_eq!(
            state.inspect(image, 1100, OwnClipboardCopy::default(), true),
            Err(CaptureSkip::DuplicateNotification)
        );
        assert!(state
            .inspect(image, 1150, OwnClipboardCopy::default(), false)
            .is_ok());
        assert!(state
            .inspect(metadata(3), 1200, OwnClipboardCopy::default(), false)
            .is_ok());
        assert!(!state.repeats_file(image));
        assert!(state
            .inspect(image, 1250, OwnClipboardCopy::default(), true)
            .is_ok());
        assert!(state
            .inspect(other, 1300, OwnClipboardCopy::default(), false)
            .is_ok());
        assert!(state
            .inspect(image, 1350, OwnClipboardCopy::default(), true)
            .is_ok());
    }

    #[test]
    fn ordinary_image_after_intervening_text_is_not_permanently_suppressed() {
        let image = CaptureReadMetadata {
            image: true,
            ..metadata(1)
        };
        let mut state = CaptureNotificationState::default();
        assert!(state
            .inspect(image, 1000, OwnClipboardCopy::default(), false)
            .is_ok());
        assert!(state
            .inspect(metadata(2), 1100, OwnClipboardCopy::default(), false)
            .is_ok());
        assert!(state
            .inspect(image, 1200, OwnClipboardCopy::default(), false)
            .is_ok());
    }

    #[test]
    fn changing_persistence_destination_clears_payload_suppression_but_keeps_sequence() {
        let mut state = CaptureNotificationState::at_sequence(4);
        let file = CaptureReadMetadata { files: true, ..metadata(1) };
        state.set_persistence_mode(true);
        assert!(state.inspect(file, 1000, OwnClipboardCopy::default(), false).is_ok());
        state.set_persistence_mode(true);
        assert!(state.repeats_file(file));
        state.set_persistence_mode(false);
        assert!(!state.repeats_file(file));
        assert!(!state.accept_sequence(4));
        assert!(state.inspect(file, 1050, OwnClipboardCopy::default(), false).is_ok());
        state.set_persistence_mode(true);
        assert!(!state.repeats_file(file));
        assert!(state.inspect(file, 1100, OwnClipboardCopy::default(), true).is_ok());
    }
}
