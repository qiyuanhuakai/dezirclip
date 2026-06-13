use std::sync::atomic::{AtomicBool, Ordering};

static EXPLICIT_APP_EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn request_app_exit() {
    EXPLICIT_APP_EXIT_REQUESTED.store(true, Ordering::SeqCst);
}

pub fn should_prevent_current_exit_requested() -> bool {
    should_prevent_exit_requested(EXPLICIT_APP_EXIT_REQUESTED.load(Ordering::SeqCst))
}

pub fn should_prevent_exit_requested(explicit_exit_requested: bool) -> bool {
    !explicit_exit_requested
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevents_runtime_exit_when_no_explicit_exit_was_requested() {
        assert!(should_prevent_exit_requested(false));
    }

    #[test]
    fn allows_runtime_exit_when_user_requested_exit() {
        assert!(!should_prevent_exit_requested(true));
    }
}
