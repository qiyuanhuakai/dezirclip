use dzc_host::wire::{RuntimeState, SurfacePaste};
pub(super) fn restores_target(reason: &str, request: u64, current: u64) -> bool {
    reason == "cancel" && request != u64::MAX && request == current
}

#[derive(Clone, Debug)]
pub(super) struct Display {
    pub id: String,
    pub state: RuntimeState,
    pub revision: u64,
    pub shown: bool,
}
#[derive(Default)]
pub(super) struct View {
    pub display: Option<Display>,
    actions: std::collections::HashSet<String>,
}
impl View {
    pub fn exhausted(&self) -> bool {
        self.actions.len() >= 256
    }
    pub fn show(&mut self, display: Display) {
        self.display = Some(display);
    }
    pub fn cancel(&mut self) -> Option<Display> {
        self.display.take()
    }
    pub fn complete(&mut self, id: &str, revision: u64) -> bool {
        if self.display.as_ref().is_some_and(|display| {
            display.id == id && display.revision == revision && !display.shown
        }) {
            self.display = None;
            return true;
        }
        false
    }
    pub fn cancel_matching(&mut self, id: &str) -> Option<Display> {
        if self
            .display
            .as_ref()
            .is_some_and(|display| display.id == id)
        {
            self.cancel()
        } else {
            None
        }
    }
    pub fn matches(&self, id: &str, revision: u64, state: &RuntimeState) -> bool {
        self.display.as_ref().is_some_and(|display| {
            display.id == id
                && display.revision == revision
                && dzc_host::same_surface_state(&display.state, state)
        })
    }
    pub fn presented(&mut self, id: &str, revision: u64, state: &RuntimeState) -> bool {
        if !self.matches(id, revision, state) {
            return false;
        }
        if let Some(display) = self.display.as_mut() {
            display.shown = true;
            true
        } else {
            false
        }
    }
    pub fn admit(
        &mut self,
        session: &str,
        revision: u64,
        request: &SurfacePaste,
        actual: &RuntimeState,
    ) -> Result<Display, String> {
        let display = self.display.as_ref().ok_or("SurfaceDisplayCancelled")?;
        if !display.shown
            || request.surface_session_id != session
            || request.display_request_id != display.id
            || display.revision != revision
            || display.state.service_instance_id != request.state.service_instance_id
            || display.state.privacy_epoch != request.state.privacy_epoch
            || !dzc_host::same_surface_state(actual, &request.state)
            || request.entry_id == 0
        {
            return Err("SurfaceSelectionChanged".into());
        }
        if self.actions.len() >= 256 || !self.actions.insert(request.action_id.clone()) {
            return Err("SurfaceActionRepeated".into());
        }
        let selected = display.clone();
        if let Some(display) = self.display.as_mut() {
            display.shown = false;
        }
        Ok(selected)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> RuntimeState {
        RuntimeState {
            service_instance_id: "owner".into(),
            privacy_epoch: 5,
            history_revision: 7,
            event_sequence: 8,
        }
    }
    fn display() -> Display {
        Display {
            id: "display-1".into(),
            state: state(),
            revision: 2,
            shown: false,
        }
    }
    fn action() -> SurfacePaste {
        SurfacePaste {
            surface_session_id: "child-1".into(),
            display_request_id: "display-1".into(),
            action_id: "action-1".into(),
            entry_id: 3,
            state: state(),
        }
    }
    #[test]
    fn pending_or_cancelled_or_old_ack_never_becomes_visible() {
        let mut view = View::default();
        view.show(display());
        assert!(view.admit("child-1", 2, &action(), &state()).is_err());
        assert!(!view.presented("display-0", 2, &state()));
        assert!(!view.presented("display-1", 3, &state()));
        view.cancel();
        assert!(!view.presented("display-1", 2, &state()));
    }
    #[test]
    fn a_matching_visibility_ack_stays_pending_until_native_focus_is_confirmed() {
        let mut view = View::default();
        view.show(display());
        assert!(view.matches("display-1", 2, &state()));
        assert!(!view.display.as_ref().unwrap().shown);
        assert!(view.admit("child-1", 2, &action(), &state()).is_err());
        assert!(!view.matches("display-1", 3, &state()));
        let mut changed = state();
        changed.privacy_epoch += 1;
        assert!(!view.matches("display-1", 2, &changed));
        assert!(view.presented("display-1", 2, &state()));
        assert!(view.admit("child-1", 2, &action(), &state()).is_ok());
        view.show(display());
        assert!(view.matches("display-1", 2, &state()));
        view.cancel();
        assert!(!view.presented("display-1", 2, &state()));
        assert!(view.admit("child-1", 2, &action(), &state()).is_err());
    }
    #[test]
    fn action_is_exactly_once_and_both_id_signs_require_original_owner_epoch_revision() {
        for id in [-3, 3] {
            for field in 0..6 {
                let mut view = View::default();
                view.show(display());
                assert!(view.presented("display-1", 2, &state()));
                let mut request = action();
                request.entry_id = id;
                match field {
                    0 => request.surface_session_id = "old-child".into(),
                    1 => request.display_request_id = "old-display".into(),
                    2 => request.state.service_instance_id = "replacement".into(),
                    3 => request.state.privacy_epoch += 1,
                    4 => request.state.history_revision += 1,
                    _ => (),
                }
                if field < 5 {
                    assert!(view.admit("child-1", 2, &request, &state()).is_err());
                } else {
                    assert!(view.admit("child-1", 2, &request, &state()).is_ok());
                    assert!(view.admit("child-1", 2, &request, &state()).is_err());
                    view.show(Display {
                        shown: true,
                        ..display()
                    });
                    assert!(view.admit("child-1", 2, &request, &state()).is_err());
                }
            }
        }
    }
    #[test]
    fn a_fresh_query_can_select_a_new_history_revision_without_rebasing_owner_or_privacy() {
        let mut view = View::default();
        view.show(Display {
            shown: true,
            ..display()
        });
        let mut request = action();
        request.state.history_revision += 1;
        assert!(view.admit("child-1", 2, &request, &state()).is_err());
        assert!(view.admit("child-1", 2, &request, &request.state).is_ok());
    }
    #[test]
    fn dismiss_can_still_cancel_an_admitted_action_and_old_completion_preserves_new_display() {
        let mut view = View::default();
        view.show(Display {
            shown: true,
            ..display()
        });
        view.admit("child-1", 2, &action(), &state()).unwrap();
        assert!(view.cancel().is_some());
        view.show(Display {
            id: "new-display".into(),
            shown: true,
            ..display()
        });
        view.complete("display-1", 2);
        assert_eq!(view.display.unwrap().id, "new-display");
    }
    #[test]
    fn blur_or_a_superseded_or_exhausted_request_never_restores_focus() {
        assert!(restores_target("cancel", 2, 2));
        assert!(!restores_target("blur", 2, 2));
        assert!(!restores_target("cancel", 2, 3));
        assert!(!restores_target("unknown", 2, 2));
        assert!(!restores_target("cancel", u64::MAX, u64::MAX));
    }
    #[test]
    fn old_completion_and_dismiss_preserve_the_new_selection_and_target_as_one_owner() {
        let mut view = View::default();
        view.show(Display {
            id: "new-display".into(),
            revision: 3,
            shown: true,
            ..display()
        });
        let mut target = Some(99);
        assert!(!super::complete_selection(
            &mut view,
            &mut target,
            "display-1",
            2
        ));
        assert!(view.cancel_matching("display-1").is_none());
        assert_eq!(target, Some(99));
        assert_eq!(view.display.as_ref().unwrap().id, "new-display");
        view.display.as_mut().unwrap().shown = false;
        assert!(super::complete_selection(
            &mut view,
            &mut target,
            "new-display",
            3
        ));
        assert_eq!(target, None);
        assert!(view.display.is_none());
    }
}
pub(super) fn complete_selection<T>(
    view: &mut View,
    target: &mut Option<T>,
    id: &str,
    revision: u64,
) -> bool {
    if view.complete(id, revision) {
        target.take();
        true
    } else {
        false
    }
}
