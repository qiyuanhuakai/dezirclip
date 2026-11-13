package main

import "testing"

func TestEmptyWaylandEnvironmentKeepsX11Interactions(t *testing.T) {
	for _, test := range []struct {
		display, socket, session string
		wayland                  bool
	}{
		{"", "", "x11", false}, {"", "", "", false}, {"", "", "Wayland", true},
		{"wayland-0", "", "x11", true}, {"", "3", "x11", true},
	} {
		t.Setenv("WAYLAND_DISPLAY", test.display)
		t.Setenv("WAYLAND_SOCKET", test.socket)
		t.Setenv("XDG_SESSION_TYPE", test.session)
		if actual := nativeWaylandRequested(); actual != test.wayland {
			t.Fatalf("session detection: got %v, want %v", actual, test.wayland)
		}
	}
}
