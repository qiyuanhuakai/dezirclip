package main

import "testing"

func TestHostReplacementPreservesCancelledAndSupersedingActivation(t *testing.T) {
	for command, want := range map[string]string{"": "minimized", "tray-show:2": "show", "settings": "settings", "search": "search"} {
		if got := recoveredActivation(command); got != want {
			t.Fatalf("recovered %q as %q, want %q", command, got, want)
		}
	}
}

func TestTrayToggleCancelsDelayedShowAndPreservesNewerIntent(t *testing.T) {
	var state trayToggleState
	first, hide := state.toggle(false)
	if hide || first == 0 {
		t.Fatal("hidden window must request an authenticated show")
	}
	if _, hide = state.toggle(false); !hide {
		t.Fatal("second click must cancel pending show")
	}
	if state.accept(first) {
		t.Fatal("cancelled show was accepted")
	}
	next, hide := state.toggle(false)
	if hide || state.accept(first) || !state.accept(next) || state.accept(next) {
		t.Fatal("show ownership was not one-shot")
	}
	if _, hide = state.toggle(true); !hide {
		t.Fatal("visible unfocused window must hide")
	}
	next, _ = state.toggle(false)
	state.cancel()
	if state.accept(next) {
		t.Fatal("older tray request survived a different activation")
	}
}
