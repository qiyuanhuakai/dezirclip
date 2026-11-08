package main

import (
	"errors"
	"os"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/egoist/mygo"
	"golang.org/x/sys/unix"
)

type mouseCookie struct{ Sequence uint32 }
type mouseScreens struct {
	Data             uintptr
	Remaining, Index int32
}
type mouseXCB struct {
	connect    func(uintptr, *int32) uintptr
	disconnect func(uintptr)
	failed     func(uintptr) int32
	setup      func(uintptr) uintptr
	screens    func(uintptr) mouseScreens
	nextScreen func(*mouseScreens)
	grab       func(uintptr, uint8, uint32, uint16, uint8, uint8, uint32, uint32, uint8, uint16) mouseCookie
	check      func(uintptr, mouseCookie) uintptr
	fd         func(uintptr) int32
	event      func(uintptr) uintptr
	free       func(uintptr)
}

var mouseXCBOnce sync.Once
var mouseXCBAPI mouseXCB
var mouseXCBError error

func loadMouseXCB() (*mouseXCB, error) {
	mouseXCBOnce.Do(func() {
		lib, err := purego.Dlopen("libxcb.so.1", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			mouseXCBError = err
			return
		}
		libc, err := purego.Dlopen("libc.so.6", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			mouseXCBError = err
			return
		}
		for name, fn := range map[string]any{
			"xcb_connect": &mouseXCBAPI.connect, "xcb_disconnect": &mouseXCBAPI.disconnect,
			"xcb_connection_has_error": &mouseXCBAPI.failed, "xcb_get_setup": &mouseXCBAPI.setup,
			"xcb_setup_roots_iterator": &mouseXCBAPI.screens, "xcb_screen_next": &mouseXCBAPI.nextScreen,
			"xcb_grab_button_checked": &mouseXCBAPI.grab, "xcb_request_check": &mouseXCBAPI.check,
			"xcb_get_file_descriptor": &mouseXCBAPI.fd, "xcb_poll_for_event": &mouseXCBAPI.event,
		} {
			purego.RegisterLibFunc(fn, lib, name)
		}
		purego.RegisterLibFunc(&mouseXCBAPI.free, libc, "free")
	})
	return &mouseXCBAPI, mouseXCBError
}

type middleRegistration struct {
	stop int
	done chan struct{}
}

var middleActive *middleRegistration

func registerMiddleShortcut(pressed func()) error {
	services, exists := os.LookupEnv("DEZIRCLIP_DISABLE_LINUX_SERVICES")
	if !exists {
		services = os.Getenv("TIEZ_DISABLE_LINUX_SERVICES")
	}
	for _, service := range strings.FieldsFunc(services, func(r rune) bool { return r == ',' || r == ';' || r == ' ' }) {
		if strings.EqualFold(service, "mouse_hotkey") || strings.EqualFold(service, "all") {
			return errors.New("MiddleShortcutDisabled")
		}
	}
	_, wayland := os.LookupEnv("WAYLAND_DISPLAY")
	_, socket := os.LookupEnv("WAYLAND_SOCKET")
	if wayland || socket || strings.EqualFold(os.Getenv("XDG_SESSION_TYPE"), "wayland") {
		return errors.New("MiddleShortcutRequiresX11")
	}
	if middleActive != nil {
		return errors.New("MiddleShortcutAlreadyRegistered")
	}
	x, err := loadMouseXCB()
	if err != nil {
		return err
	}
	var screen int32
	connection := x.connect(0, &screen)
	if connection == 0 {
		return errors.New("MouseDisplayUnavailable")
	}
	ready := false
	defer func() {
		if !ready {
			x.disconnect(connection)
		}
	}()
	if x.failed(connection) != 0 {
		return errors.New("MouseDisplayUnavailable")
	}
	screens := x.screens(x.setup(connection))
	for screen > 0 && screens.Remaining > 0 {
		x.nextScreen(&screens)
		screen--
	}
	if screens.Remaining <= 0 || screens.Data == 0 {
		return errors.New("MouseScreenUnavailable")
	}
	root := *(*uint32)(unsafe.Pointer(screens.Data))
	// Checked grabs report conflicts without a process-wide Xlib error handler.
	cookie := x.grab(connection, 0, root, 4|8, 1, 1, 0, 0, 2, 32768)
	if nativeError := x.check(connection, cookie); nativeError != 0 {
		x.free(nativeError)
		return errors.New("MiddleShortcutUnavailable")
	}
	fds, err := unix.Socketpair(unix.AF_UNIX, unix.SOCK_STREAM|unix.SOCK_CLOEXEC, 0)
	if err != nil {
		return err
	}
	registration := &middleRegistration{stop: fds[1], done: make(chan struct{})}
	middleActive = registration
	ready = true
	go func() {
		delivery := make(chan struct{}, 1)
		defer close(registration.done)
		defer unix.Close(fds[0])
		defer x.disconnect(connection)
		poll := []unix.PollFd{{Fd: x.fd(connection), Events: unix.POLLIN}, {Fd: int32(fds[0]), Events: unix.POLLIN}}
		for {
			for event := x.event(connection); event != 0; event = x.event(connection) {
				header := (*[2]byte)(unsafe.Pointer(event))
				button := middleButtonReleased(header[0], header[1])
				x.free(event)
				if button {
					select {
					case delivery <- struct{}{}:
						go func() {
							defer func() { <-delivery }()
							mygo.RunOnMain(func() {
								if middleActive == registration {
									pressed()
								}
							})
						}()
					default:
					}
				}
			}
			if x.failed(connection) != 0 {
				return
			}
			if _, err := unix.Poll(poll, -1); err != nil {
				if err == unix.EINTR {
					continue
				}
				return
			}
			if poll[1].Revents != 0 || poll[0].Revents&(unix.POLLERR|unix.POLLHUP|unix.POLLNVAL) != 0 {
				return
			}
		}
	}()
	return nil
}

func unregisterMiddleShortcut() {
	registration := middleActive
	if registration == nil {
		return
	}
	middleActive = nil
	_, _ = unix.Write(registration.stop, []byte{1})
	<-registration.done
	unix.Close(registration.stop)
}
