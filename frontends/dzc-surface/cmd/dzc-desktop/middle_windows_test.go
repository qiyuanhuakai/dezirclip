package main

import (
	"errors"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"golang.org/x/sys/windows"
)

func middleHookFixtureEnabled() bool {
	return os.Getenv("GITHUB_ACTIONS") == "true" && os.Getenv("RUNNER_ENVIRONMENT") == "github-hosted" && os.Getenv("DZC_TEST_MIDDLE_HOOK") == "1"
}

// App.Run requires the initial goroutine, whereas testing runs each Test in a
// worker goroutine. Keep the native loop in TestMain and propagate its result.
func TestMain(m *testing.M) {
	if middleHookFixtureEnabled() {
		if err := runMiddleHookFixture(); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
	os.Exit(m.Run())
}

func TestMiddleHookSurvivesBlockedResident(t *testing.T) {
	if !middleHookFixtureEnabled() {
		t.Skip("requires disposable Windows desktop")
	}
}

func runMiddleHookFixture() error {
	finished := make(chan error, 1)
	mygo.App.WhenReady(func() {
		pressed := make(chan struct{}, 2)
		if err := registerMiddleShortcut(func() { pressed <- struct{}{} }); err != nil {
			finished <- err
			mygo.App.Quit()
			return
		}
		registration := middleActive
		if registration.thread == windows.GetCurrentThreadId() {
			finished <- errors.New("hook installed on resident UI thread")
			unregisterMiddleShortcut()
			mygo.App.Quit()
			return
		}
		release := make(chan struct{})
		go func() {
			var failure error
			defer func() {
				mygo.RunOnMain(func() { unregisterMiddleShortcut(); mygo.App.Quit() })
				finished <- failure
			}()
			mouse := mouseUser32.NewProc("mouse_event")
			mouse.Call(0x20, 0, 0, 0, 0)
			time.Sleep(200 * time.Millisecond)
			if len(registration.delivery) != 0 {
				failure = errors.New("delivered before button release")
			}
			mouse.Call(0x40, 0, 0, 0, 0)
			deadline := time.Now().Add(2 * time.Second)
			for len(registration.delivery) == 0 && time.Now().Before(deadline) {
				time.Sleep(10 * time.Millisecond)
			}
			if len(registration.delivery) != 1 {
				failure = errors.New("hook did not receive release while UI was blocked")
			}
			// Exceed Windows' maximum low-level hook timeout while its installing
			// thread continues pumping independently of this blocked UI callback.
			time.Sleep(1500 * time.Millisecond)
			close(release)
			select {
			case <-pressed:
			case <-time.After(2 * time.Second):
				failure = errors.New("queued release was lost")
				return
			}
			deadline = time.Now().Add(time.Second)
			for len(registration.delivery) != 0 && time.Now().Before(deadline) {
				time.Sleep(time.Millisecond)
			}
			mouse.Call(0x20, 0, 0, 0, 0)
			mouse.Call(0x40, 0, 0, 0, 0)
			select {
			case <-pressed:
			case <-time.After(2 * time.Second):
				failure = errors.New("Windows removed the hook during UI stall")
			}
		}()
		<-release
	})
	if err := mygo.App.Run(); err != nil {
		return err
	}
	return <-finished
}
