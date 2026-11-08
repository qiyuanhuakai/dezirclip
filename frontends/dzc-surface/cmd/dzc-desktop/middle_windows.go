package main

import (
	"errors"
	"syscall"

	"github.com/egoist/mygo"
	"golang.org/x/sys/windows"
)

// Registration and delivery belong to the resident's native UI thread. The
// low-level callback only queues work; it never waits for Rust or a WebView.
var middleHook uintptr
var middlePressed func()
var middleGeneration uint64
var middleDelivery chan struct{}
var mouseUser32 = windows.NewLazySystemDLL("user32.dll")
var mouseCallNext = mouseUser32.NewProc("CallNextHookEx")
var middleCallback = syscall.NewCallback(func(code int32, message uintptr, data uintptr) uintptr {
	if code >= 0 && middleHook != 0 && (message == 0x207 || message == 0x208) {
		if message == 0x207 {
			generation := middleGeneration
			delivery := middleDelivery
			select {
			case delivery <- struct{}{}:
				go func() {
					defer func() { <-delivery }()
					mygo.RunOnMain(func() {
						if generation == middleGeneration && middlePressed != nil {
							middlePressed()
						}
					})
				}()
			default:
			}
		}
		return 1
	}
	result, _, _ := mouseCallNext.Call(0, uintptr(code), message, data)
	return result
})

func registerMiddleShortcut(pressed func()) error {
	if middleHook != 0 {
		return errors.New("MiddleShortcutAlreadyRegistered")
	}
	module, _, err := windows.NewLazySystemDLL("kernel32.dll").NewProc("GetModuleHandleW").Call(0)
	if module == 0 {
		return err
	}
	hook, _, err := mouseUser32.NewProc("SetWindowsHookExW").Call(14, middleCallback, module, 0)
	if hook == 0 {
		return err
	}
	middleGeneration++
	middleDelivery = make(chan struct{}, 1)
	middlePressed, middleHook = pressed, hook
	return nil
}

func unregisterMiddleShortcut() {
	if middleHook == 0 {
		return
	}
	middleGeneration++
	middlePressed = nil
	mouseUser32.NewProc("UnhookWindowsHookEx").Call(middleHook)
	middleHook = 0
}
