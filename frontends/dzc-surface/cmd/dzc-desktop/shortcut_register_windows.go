package main

import (
	"errors"
	"log"
	"runtime"
	"strings"
	"sync/atomic"
	"time"
	"unsafe"

	"github.com/egoist/mygo"
	"golang.org/x/sys/windows"
)

type extraWindowsRegistration struct {
	thread uint32
	active atomic.Bool
	done   chan struct{}
}

var extraWindowsRegistrations = map[string]*extraWindowsRegistration{} // Resident UI thread.

func extraWindowsKey(name string) uintptr {
	switch name {
	case "Pause":
		return 0x13
	case "ContextMenu":
		return 0x5d
	default:
		return 0
	}
}

// These keys use Win32 directly because MyGo's accelerator parser omits them.
func registerExtraShortcut(chord string, pressed func()) (bool, error) {
	parts := strings.Split(chord, "+")
	keyName := parts[len(parts)-1]
	if keyName == "F12" {
		return true, errors.New("ShortcutUnavailable")
	}
	key := extraWindowsKey(keyName)
	if key == 0 {
		return false, nil
	}
	modifiers := uintptr(0x4000) // MOD_NOREPEAT
	for _, part := range parts[:len(parts)-1] {
		switch part {
		case "Alt":
			modifiers |= 1
		case "Ctrl":
			modifiers |= 2
		case "Shift":
			modifiers |= 4
		case "Super":
			modifiers |= 8
		default:
			return true, errors.New("InvalidShortcut")
		}
	}
	if extraWindowsRegistrations[chord] != nil {
		return true, errors.New("ShortcutAlreadyRegistered")
	}
	registration := &extraWindowsRegistration{done: make(chan struct{})}
	ready := make(chan error, 1)
	go registration.run(key, modifiers, pressed, ready)
	if err := <-ready; err != nil {
		<-registration.done
		return true, err
	}
	extraWindowsRegistrations[chord] = registration
	return true, nil
}

func (r *extraWindowsRegistration) run(key, modifiers uintptr, pressed func(), ready chan<- error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	defer close(r.done)

	var message windowMessage
	r.thread = windows.GetCurrentThreadId()
	shortcutUser32.NewProc("PeekMessageW").Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0, 0)
	const hotkeyID, wmHotkey = 1, 0x0312
	if registered, _, err := shortcutUser32.NewProc("RegisterHotKey").Call(0, hotkeyID, modifiers, key); registered == 0 {
		ready <- err
		return
	}
	defer shortcutUser32.NewProc("UnregisterHotKey").Call(0, hotkeyID)
	r.active.Store(true)
	defer r.active.Store(false)
	ready <- nil

	getMessage := shortcutUser32.NewProc("GetMessageW")
	delivery := make(chan struct{}, 1)
	for r.active.Load() {
		result, _, err := getMessage.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if int32(result) == -1 {
			log.Printf("Extended shortcut message loop: %v", err)
		}
		if int32(result) <= 0 {
			return
		}
		if message.Message == wmHotkey && message.WParam == hotkeyID {
			select {
			case delivery <- struct{}{}:
				go func() {
					defer func() { <-delivery }()
					mygo.RunOnMain(func() {
						if r.active.Load() {
							pressed()
						}
					})
				}()
			default:
			}
		}
	}
}

func unregisterExtraShortcut(chord string) bool {
	registration := extraWindowsRegistrations[chord]
	if registration == nil {
		return false
	}
	registration.active.Store(false)
	const wmQuit = 0x0012
	shortcutUser32.NewProc("PostThreadMessageW").Call(uintptr(registration.thread), wmQuit, 0, 0)
	select {
	case <-registration.done:
		delete(extraWindowsRegistrations, chord)
	case <-time.After(2 * time.Second):
		log.Printf("Extended shortcut shutdown timed out for %s", chord)
	}
	return true
}
