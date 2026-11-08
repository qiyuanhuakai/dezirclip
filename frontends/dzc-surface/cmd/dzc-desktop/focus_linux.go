package main

import (
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/egoist/mygo"
)

// A resident's X11 keyboard grab temporarily clears GTK's active flag in the
// separate WebView process. The window manager's active window remains stable.
var x11MainActive = sync.OnceValue(func() func(uintptr) (bool, bool) {
	gtk, err := purego.Dlopen("libgtk-3.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil
	}
	gdk, err := purego.Dlopen("libgdk-3.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil
	}
	object, err := purego.Dlopen("libgobject-2.0.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil
	}
	xlib, err := purego.Dlopen("libX11.so.6", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil
	}
	var nativeWindow, display, screen, root func(uintptr) uintptr
	var displayType func() uintptr
	var isType func(uintptr, uintptr) int32
	var xdisplay func(uintptr) uintptr
	var xid func(uintptr) uint64
	var atom func(uintptr, string, int32) uint64
	var property func(uintptr, uint64, uint64, int64, int64, int32, uint64, *uint64, *int32, *uint64, *uint64, *uintptr) int32
	var free func(uintptr) int32
	for _, binding := range []struct {
		library  uintptr
		name     string
		function any
	}{
		{gtk, "gtk_widget_get_window", &nativeWindow},
		{gdk, "gdk_window_get_display", &display},
		{gdk, "gdk_window_get_screen", &screen},
		{gdk, "gdk_screen_get_root_window", &root},
		{gdk, "gdk_x11_display_get_type", &displayType},
		{object, "g_type_check_instance_is_a", &isType},
		{gdk, "gdk_x11_display_get_xdisplay", &xdisplay},
		{gdk, "gdk_x11_window_get_xid", &xid},
		{xlib, "XInternAtom", &atom},
		{xlib, "XGetWindowProperty", &property},
		{xlib, "XFree", &free},
	} {
		address, err := purego.Dlsym(binding.library, binding.name)
		if err != nil {
			return nil
		}
		purego.RegisterFunc(binding.function, address)
	}
	return func(handle uintptr) (bool, bool) {
		window := nativeWindow(handle)
		if window == 0 {
			return false, false
		}
		d := display(window)
		if d == 0 || isType(d, displayType()) == 0 {
			return false, false
		}
		x := xdisplay(d)
		r := root(screen(window))
		if x == 0 || r == 0 {
			return false, false
		}
		name := atom(x, "_NET_ACTIVE_WINDOW", 1)
		if name == 0 {
			return false, false
		}
		var actualType, count, remaining uint64
		var format int32
		var value uintptr
		status := property(x, xid(r), name, 0, 1, 0, 33, &actualType, &format, &count, &remaining, &value)
		if value != 0 {
			defer free(value)
		}
		if status != 0 || actualType != 33 || format != 32 || count != 1 || value == 0 {
			return false, false
		}
		// Xlib expands format-32 values to native unsigned long slots on LP64.
		return *(*uint64)(unsafe.Pointer(value)) == xid(window), true
	}
})

func mainWindowFocused(w *mygo.Window) bool {
	focused := false
	mygo.RunOnMain(func() {
		if w.IsDestroyed() || !w.IsVisible() {
			return
		}
		if query := x11MainActive(); query != nil {
			if active, supported := query(w.NativeHandle()); supported {
				focused = active
				return
			}
		}
		focused = w.IsFocused()
	})
	return focused
}
