package main

import (
	"errors"
	"log"
	"runtime"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
	"unsafe"

	"github.com/egoist/mygo"
	"golang.org/x/sys/windows"
)

type mouseMessage struct {
	Window         uintptr
	Message        uint32
	WParam, LParam uintptr
	Time           uint32
	X, Y           int32
	Private        uint32
}

type middleRegistration struct {
	thread    uint32
	cancelled atomic.Bool
	done      chan struct{}
	delivery  chan struct{}
	pressed   func()
}

var middleActive *middleRegistration // Resident UI thread only.
var middleThreads sync.Map
var mouseUser32 = windows.NewLazySystemDLL("user32.dll")
var mouseCallNext = mouseUser32.NewProc("CallNextHookEx")

const stopMouseThread = 0x8000 + 0x417

// Windows delivers a low-level hook on its installing thread. This thread must
// pump messages even while the resident's UI waits for a Host response.
var middleCallback = syscall.NewCallback(func(code int32, message uintptr, data uintptr) uintptr {
	if value, ok := middleThreads.Load(windows.GetCurrentThreadId()); ok && code >= 0 {
		registration := value.(*middleRegistration)
		if !registration.cancelled.Load() && (message == 0x207 || message == 0x208) {
			if message == 0x208 {
				select {
				case registration.delivery <- struct{}{}:
					go func() {
						defer func() { <-registration.delivery }()
						mygo.RunOnMain(func() {
							if !registration.cancelled.Load() {
								registration.pressed()
							}
						})
					}()
				default:
				}
			}
			return 1
		}
	}
	result, _, _ := mouseCallNext.Call(0, uintptr(code), message, data)
	return result
})

func registerMiddleShortcut(pressed func()) error {
	if middleActive != nil {
		return errors.New("MiddleShortcutAlreadyRegistered")
	}
	registration := &middleRegistration{done: make(chan struct{}), delivery: make(chan struct{}, 1), pressed: pressed}
	ready := make(chan error, 1)
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		defer close(registration.done)
		registration.thread = windows.GetCurrentThreadId()
		var message mouseMessage
		mouseUser32.NewProc("PeekMessageW").Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0, 0)
		module, _, err := windows.NewLazySystemDLL("kernel32.dll").NewProc("GetModuleHandleW").Call(0)
		if module == 0 {
			ready <- err
			return
		}
		middleThreads.Store(registration.thread, registration)
		defer middleThreads.Delete(registration.thread)
		hook, _, err := mouseUser32.NewProc("SetWindowsHookExW").Call(14, middleCallback, module, 0)
		if hook == 0 {
			ready <- err
			return
		}
		defer mouseUser32.NewProc("UnhookWindowsHookEx").Call(hook)
		ready <- nil
		getMessage := mouseUser32.NewProc("GetMessageW")
		for {
			result, _, err := getMessage.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
			if int32(result) == -1 {
				log.Printf("Middle shortcut message loop failed: %v", err)
				return
			}
			if result == 0 || message.Message == stopMouseThread || registration.cancelled.Load() {
				return
			}
		}
	}()
	if err := <-ready; err != nil {
		<-registration.done
		return err
	}
	middleActive = registration
	return nil
}

func unregisterMiddleShortcut() {
	registration := middleActive
	if registration == nil {
		return
	}
	registration.cancelled.Store(true)
	middleActive = nil
	result, _, err := mouseUser32.NewProc("PostThreadMessageW").Call(uintptr(registration.thread), stopMouseThread, 0, 0)
	if result == 0 {
		log.Printf("Middle shortcut cancellation failed: %v", err)
	}
	select {
	case <-registration.done:
	case <-time.After(2 * time.Second):
		log.Print("Middle shortcut shutdown timed out; delivery disabled")
	}
}
