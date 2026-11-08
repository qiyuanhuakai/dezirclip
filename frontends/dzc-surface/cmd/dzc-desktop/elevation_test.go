package main

import (
	"os"
	"testing"
)

func TestIncompleteElevationArgumentsCannotStartHandoff(t *testing.T) {
	previous := os.Args
	t.Cleanup(func() { os.Args = previous })
	for _, mode := range []string{"--elevate-helper", "--after-elevation"} {
		os.Args = []string{previous[0], mode}
		handled, err := prepareElevation()
		if !handled || err == nil {
			t.Fatalf("incomplete %s was not rejected", mode)
		}
	}
	os.Args = []string{previous[0], "--show"}
	if handled, err := prepareElevation(); handled || err != nil {
		t.Fatalf("ordinary launch changed: %v", err)
	}
}
