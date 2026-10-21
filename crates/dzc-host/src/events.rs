use dzc_protocol::wire::{Event, EventData, EventKind, EventsResult, RuntimeState};
use std::{collections::VecDeque, sync::{Condvar, Mutex}, time::Duration};

pub(crate) struct Events { state: Mutex<EventState>, changed: Condvar }
struct EventState { sequence: u64, recent: VecDeque<Event>, interruption: u64,
    #[cfg(test)] waiters: usize,
}
impl Events {
    pub fn new() -> Self { Self { state: Mutex::new(EventState { sequence: 0, recent: VecDeque::new(), interruption: 0,
        #[cfg(test)] waiters: 0,
    }), changed: Condvar::new() } }
    #[cfg(test)]
    pub fn waiters(&self) -> usize { self.state.lock().unwrap().waiters }
    pub fn interrupt(&self) {
        if let Ok(mut state) = self.state.lock() { state.interruption = state.interruption.wrapping_add(1); self.changed.notify_all(); }
    }
    pub fn sequence(&self) -> Result<u64, ()> { self.state.lock().map(|state| state.sequence).map_err(|_| ()) }
    pub fn push(&self, kind: EventKind, mut snapshot: RuntimeState, data: EventData) -> Result<(), ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        state.sequence = state.sequence.checked_add(1).filter(|next| *next <= 9_007_199_254_740_991).ok_or(())?;
        snapshot.event_sequence = state.sequence;
        state.recent.push_back(Event { event: kind, state: snapshot, data });
        if state.recent.len() > 256 { state.recent.pop_front(); }
        self.changed.notify_all();
        Ok(())
    }
    pub fn after(&self, after: u64, wait: Duration, mut snapshot: RuntimeState) -> Result<EventsResult, ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.sequence == after && !wait.is_zero() {
            let interruption = state.interruption;
            #[cfg(test)] { state.waiters += 1; }
            state = self.changed.wait_timeout_while(state, wait, |state| state.sequence == after && state.interruption == interruption).map_err(|_| ())?.0;
            #[cfg(test)] { state.waiters -= 1; }
        }
        let first = state.recent.front().map(|event| event.state.event_sequence).unwrap_or(1);
        let resync_required = after > state.sequence || after.saturating_add(1) < first;
        snapshot.event_sequence = state.sequence;
        Ok(EventsResult { events: if resync_required { vec![] } else { state.recent.iter().filter(|event| event.state.event_sequence > after).cloned().collect() }, state: snapshot, resync_required })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_window_is_bounded_and_gaps_require_refresh() {
        let events = Events::new();
        let snapshot = RuntimeState { service_instance_id: "synthetic".into(), history_revision: 300, privacy_epoch: 1, event_sequence: 0 };
        for id in 1..=300 { events.push(EventKind::EntryAdded, snapshot.clone(), EventData { entry_id: Some(id), status: None }).unwrap(); }
        assert!(events.after(0, Duration::ZERO, snapshot.clone()).unwrap().resync_required);
        let page = events.after(44, Duration::ZERO, snapshot.clone()).unwrap();
        assert_eq!(page.events.len(), 256);
        assert_eq!(page.state.event_sequence, 300);
        assert!(events.after(301, Duration::ZERO, snapshot).unwrap().resync_required);
    }
}
