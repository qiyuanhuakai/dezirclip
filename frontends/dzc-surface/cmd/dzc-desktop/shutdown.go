package main

import (
	"fmt"
	"time"
)

// Only the child process handle created by this resident may be terminated.
// The Service uses its authenticated drain protocol instead of a process kill.
func waitDesktopExit(done <-chan struct{}, kill func() error, grace time.Duration) error {
	select {
	case <-done:
		return nil
	case <-time.After(grace):
	}
	if err := kill(); err != nil {
		select {
		case <-done:
			return nil
		default:
			return fmt.Errorf("main window shutdown: %w", err)
		}
	}
	select {
	case <-done:
		return nil
	case <-time.After(grace):
		return fmt.Errorf("main window exit unconfirmed")
	}
}
