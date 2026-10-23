use std::time::Duration;

const ATTEMPTS: usize = 6;
const RETRY_DELAY: Duration = Duration::from_millis(20);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum AcquisitionFailure {
    Busy,
    SelectionChanged,
}

/// Retry only lock acquisition for one known selection. The returned guard owns
/// the lock; a successful acquisition for a different selection is dropped.
pub(super) fn acquire_same_selection<T>(
    expected: u64,
    mut current: impl FnMut() -> Option<u64>,
    mut acquire: impl FnMut() -> Option<T>,
    mut wait: impl FnMut(Duration),
) -> Result<T, AcquisitionFailure> {
    if expected == 0 {
        return Err(AcquisitionFailure::SelectionChanged);
    }
    for attempt in 0..ATTEMPTS {
        if current() != Some(expected) {
            return Err(AcquisitionFailure::SelectionChanged);
        }
        if let Some(guard) = acquire() {
            if current() != Some(expected) {
                return Err(AcquisitionFailure::SelectionChanged);
            }
            return Ok(guard);
        }
        if attempt + 1 < ATTEMPTS {
            wait(RETRY_DELAY);
        }
    }
    Err(AcquisitionFailure::Busy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn transient_contention_recovers_for_the_original_selection() {
        let attempts = Cell::new(0);
        let mut delays = Vec::new();
        let guard = acquire_same_selection(7, || Some(7), || {
            attempts.set(attempts.get() + 1);
            (attempts.get() == 3).then_some("owned-lock")
        }, |delay| delays.push(delay)).unwrap();
        assert_eq!(guard, "owned-lock");
        assert_eq!(attempts.get(), 3);
        assert_eq!(delays, [RETRY_DELAY, RETRY_DELAY]);
    }

    #[test]
    fn persistent_contention_has_a_fixed_attempt_and_wait_budget() {
        let mut attempts = 0;
        let mut delays = Vec::new();
        let result = acquire_same_selection::<()>(7, || Some(7), || {
            attempts += 1;
            None
        }, |delay| delays.push(delay));
        assert_eq!(result, Err(AcquisitionFailure::Busy));
        assert_eq!(attempts, ATTEMPTS);
        assert_eq!(delays.len(), ATTEMPTS - 1);
        assert_eq!(delays.into_iter().sum::<Duration>(), Duration::from_millis(100));
    }

    #[test]
    fn selection_replaced_during_wait_is_never_acquired() {
        let sequence = Cell::new(7);
        let mut attempts = 0;
        let result = acquire_same_selection::<()>(7, || Some(sequence.get()), || {
            attempts += 1;
            None
        }, |_| sequence.set(8));
        assert_eq!(result, Err(AcquisitionFailure::SelectionChanged));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn selection_replaced_during_acquisition_releases_the_guard() {
        struct Guard<'a>(&'a Cell<usize>);
        impl Drop for Guard<'_> {
            fn drop(&mut self) { self.0.set(self.0.get() + 1); }
        }
        let sequence = Cell::new(7);
        let releases = Cell::new(0);
        let result = acquire_same_selection(7, || Some(sequence.get()), || {
            sequence.set(8);
            Some(Guard(&releases))
        }, |_| panic!("A replaced selection must not be retried"));
        assert!(matches!(result, Err(AcquisitionFailure::SelectionChanged)));
        assert_eq!(releases.get(), 1);
    }

    #[test]
    fn unknown_or_already_replaced_selection_never_opens_the_clipboard() {
        for (expected, observed) in [(0, Some(0)), (7, None), (7, Some(8))] {
            let result = acquire_same_selection::<()>(expected, || observed,
                || panic!("An unbound selection must not open the clipboard"),
                |_| panic!("An unbound selection must not wait"));
            assert_eq!(result, Err(AcquisitionFailure::SelectionChanged));
        }
    }
}
