use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Selection {
    pub owner: usize,
    pub sequence: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Keep,
    Retry,
    ClearOriginal,
    ReleaseWithoutClear,
}

pub(super) fn action(
    original: Selection,
    current: Option<Selection>,
    revoked: bool,
    age: Duration,
    lifetime: Duration,
) -> Action {
    let Some(current) = current else {
        return Action::Retry;
    };
    if original.owner == 0 || original.sequence == 0 || current != original {
        return Action::ReleaseWithoutClear;
    }
    if revoked || age >= lifetime {
        Action::ClearOriginal
    } else {
        Action::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ORIGINAL: Selection = Selection { owner: 7, sequence: 11 };
    const LIFETIME: Duration = Duration::from_secs(300);

    #[test]
    fn current_original_stays_until_the_deadline() {
        assert_eq!(action(ORIGINAL, Some(ORIGINAL), false, Duration::from_secs(299), LIFETIME), Action::Keep);
        assert_eq!(action(ORIGINAL, Some(ORIGINAL), false, LIFETIME, LIFETIME), Action::ClearOriginal);
    }
    #[test]
    fn revoked_original_clears_without_renewing_the_lease() {
        assert_eq!(action(ORIGINAL, Some(ORIGINAL), true, Duration::ZERO, LIFETIME), Action::ClearOriginal);
    }
    #[test]
    fn later_external_owner_is_never_cleared() {
        assert_eq!(action(ORIGINAL, Some(Selection { owner: 8, sequence: 12 }), true, LIFETIME, LIFETIME), Action::ReleaseWithoutClear);
    }
    #[test]
    fn later_normal_write_with_same_owner_is_never_cleared() {
        assert_eq!(action(ORIGINAL, Some(Selection { sequence: 12, ..ORIGINAL }), true, LIFETIME, LIFETIME), Action::ReleaseWithoutClear);
    }
    #[test]
    fn changed_owner_with_same_sequence_is_never_cleared() {
        assert_eq!(action(ORIGINAL, Some(Selection { owner: 8, ..ORIGINAL }), true, LIFETIME, LIFETIME), Action::ReleaseWithoutClear);
    }
    #[test]
    fn unavailable_original_is_retried_instead_of_assumed_clear() {
        assert_eq!(action(ORIGINAL, None, true, LIFETIME, LIFETIME), Action::Retry);
    }
    #[test]
    fn invalid_original_cannot_claim_a_clipboard() {
        for original in [Selection { owner: 0, ..ORIGINAL }, Selection { sequence: 0, ..ORIGINAL }] {
            assert_eq!(action(original, Some(original), true, LIFETIME, LIFETIME), Action::ReleaseWithoutClear);
        }
    }
    #[test]
    fn stale_expiry_cannot_clear_a_replacement_lease() {
        let replacement = Selection { sequence: 12, ..ORIGINAL };
        assert_eq!(action(ORIGINAL, Some(replacement), true, LIFETIME, LIFETIME), Action::ReleaseWithoutClear);
        assert_eq!(action(replacement, Some(replacement), false, Duration::ZERO, LIFETIME), Action::Keep);
    }
}
