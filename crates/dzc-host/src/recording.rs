use dzc_protocol::wire::WireErrorCode;
use std::{sync::Mutex, time::{Duration, Instant}};

pub(crate) const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(crate) struct Recording(Mutex<Option<(String, Instant)>>);

impl Recording {
    pub(crate) fn set(&self, owner: &str, enabled: bool) -> Result<(), WireErrorCode> {
        self.set_at(owner, enabled, Instant::now())
    }

    fn set_at(&self, owner: &str, enabled: bool, now: Instant) -> Result<(), WireErrorCode> {
        let mut current = self.0.lock().map_err(|_| WireErrorCode::Unavailable)?;
        if current.as_ref().is_some_and(|(_, deadline)| *deadline <= now) { current.take(); }
        if current.as_ref().is_some_and(|(id, _)| id != owner) {
            return Err(WireErrorCode::Busy);
        }
        *current = enabled.then(|| (owner.to_owned(), now + TIMEOUT));
        Ok(())
    }

    pub(crate) fn active(&self) -> Result<bool, WireErrorCode> {
        let current = self.0.lock().map_err(|_| WireErrorCode::Unavailable)?;
        Ok(current.as_ref().is_some_and(|(_, deadline)| *deadline > Instant::now()))
    }

    pub(crate) fn disconnect(&self, owner: &str) {
        if let Ok(mut current) = self.0.lock() {
            if current.as_ref().is_some_and(|(id, _)| id == owner) { current.take(); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_has_one_owner_and_expires_without_persisting_a_setting() {
        let recording = Recording::default();
        let now = Instant::now();
        recording.set_at("first", true, now).unwrap();
        assert!(recording.active().unwrap());
        assert_eq!(recording.set_at("other", true, now), Err(WireErrorCode::Busy));
        assert_eq!(recording.set_at("other", false, now), Err(WireErrorCode::Busy));
        recording.disconnect("other");
        assert!(recording.active().unwrap());
        recording.disconnect("first");
        assert!(!recording.active().unwrap());
        recording.set_at("first", true, now - TIMEOUT).unwrap();
        assert!(!recording.active().unwrap());
        recording.set_at("other", true, now).unwrap();
        recording.set_at("other", false, now).unwrap();
        assert!(!recording.active().unwrap());
    }
}
