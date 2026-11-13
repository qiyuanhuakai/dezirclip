package main

import (
	"fmt"
	"net/url"
	"os"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"github.com/egoist/mygo"
)

type gtkFileDropState struct {
	window  *mygo.Window
	context uintptr
}

// The GTK thread owns this map and the native callbacks. Request URI data at
// drop time: WebKit's hover prefetch need not arrive before its drop signal.
var gtkFileDrops = map[uintptr]*gtkFileDropState{}
var installGTKFileDrop = sync.OnceValues(func() (func(*mygo.Window), error) {
	api, err := fileDragGTK()
	if err != nil {
		return nil, err
	}
	gtk, err := purego.Dlopen("libgtk-3.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	object, err := purego.Dlopen("libgobject-2.0.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	glib, err := purego.Dlopen("libglib-2.0.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	gdk, err := purego.Dlopen("libgdk-3.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	webkit, err := purego.Dlopen("libwebkit2gtk-4.1.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	var children func(uintptr) uintptr
	var listFree func(uintptr)
	var isType func(uintptr, uintptr) int32
	var containerType, webType func() uintptr
	var atom func(string, int32) uintptr
	var request func(uintptr, uintptr, uintptr, uint32)
	var finish func(uintptr, int32, int32, uint32)
	var stop func(uintptr, string)
	var selectionLength func(uintptr) int32
	var selectionBytes func(uintptr) uintptr
	var offeredTargets func(uintptr) uintptr
	var offeredActions func(uintptr) uint32
	var dragStatus func(uintptr, uint32, uint32)
	purego.RegisterLibFunc(&children, gtk, "gtk_container_get_children")
	purego.RegisterLibFunc(&listFree, glib, "g_list_free")
	purego.RegisterLibFunc(&isType, object, "g_type_check_instance_is_a")
	purego.RegisterLibFunc(&containerType, gtk, "gtk_container_get_type")
	purego.RegisterLibFunc(&webType, webkit, "webkit_web_view_get_type")
	purego.RegisterLibFunc(&atom, gdk, "gdk_atom_intern")
	purego.RegisterLibFunc(&request, gtk, "gtk_drag_get_data")
	purego.RegisterLibFunc(&finish, gtk, "gtk_drag_finish")
	purego.RegisterLibFunc(&stop, object, "g_signal_stop_emission_by_name")
	purego.RegisterLibFunc(&selectionLength, gtk, "gtk_selection_data_get_length")
	purego.RegisterLibFunc(&selectionBytes, gtk, "gtk_selection_data_get_data")
	purego.RegisterLibFunc(&offeredTargets, gdk, "gdk_drag_context_list_targets")
	purego.RegisterLibFunc(&offeredActions, gdk, "gdk_drag_context_get_actions")
	purego.RegisterLibFunc(&dragStatus, gdk, "gdk_drag_status")
	uriTarget := atom("text/uri-list", 0)
	accepts := func(context uintptr) bool {
		if offeredActions(context)&2 == 0 {
			return false
		}
		for current := offeredTargets(context); current != 0; {
			node := (*struct{ data, next, previous uintptr })(unsafe.Pointer(current))
			if node.data == uriTarget {
				return true
			}
			current = node.next
		}
		return false
	}
	motionCallback := purego.NewCallback(func(widget, context uintptr, x, y int32, stamp uint32, data uintptr) int32 {
		if gtkFileDrops[widget] == nil || !accepts(context) {
			return 0
		}
		dragStatus(context, 2, stamp)
		return 1
	})
	dropCallback := purego.NewCallback(func(widget, context uintptr, x, y int32, stamp uint32, data uintptr) int32 {
		state := gtkFileDrops[widget]
		if state == nil || !accepts(context) {
			return 0
		}
		state.context = context
		request(widget, context, uriTarget, stamp)
		return 1
	})
	dataCallback := purego.NewCallback(func(widget, context uintptr, x, y int32, selection uintptr, info, stamp uint32, data uintptr) {
		state := gtkFileDrops[widget]
		if state == nil || state.context != context || state.context == 0 {
			return
		}
		stop(widget, "drag-data-received")
		if api.selectionTarget(selection) != uriTarget {
			return
		}
		state.context = 0
		var paths []string
		length, address := selectionLength(selection), selectionBytes(selection)
		if length > 0 && length <= 512*1024 && address != 0 {
			body := string(unsafe.Slice((*byte)(unsafe.Pointer(address)), int(length)))
			for _, line := range strings.Split(body, "\n") {
				line = strings.TrimSuffix(line, "\r")
				if line == "" || strings.HasPrefix(line, "#") {
					continue
				}
				uri, err := url.Parse(line)
				if err != nil || uri.Scheme != "file" || (uri.Host != "" && uri.Host != "localhost") || uri.RawQuery != "" || uri.Fragment != "" {
					paths = nil
					break
				}
				paths = append(paths, uri.Path)
			}
		}
		accepted := int32(0)
		if !fileDragPending.Load() && len(paths) > 0 && len(paths) <= 256 {
			accepted = 1
			fmt.Fprintf(os.Stderr, "Native GTK file drop: %d files\n", len(paths))
			filesDropped.Emit(state.window, paths)
		}
		finish(context, accepted, 0, stamp)
	})
	destroyCallback := purego.NewCallback(func(widget, data uintptr) { delete(gtkFileDrops, widget) })
	var find func(uintptr) uintptr
	find = func(widget uintptr) uintptr {
		if isType(widget, webType()) != 0 {
			return widget
		}
		if isType(widget, containerType()) == 0 {
			return 0
		}
		list := children(widget)
		defer listFree(list)
		for current := list; current != 0; {
			node := (*struct{ data, next, previous uintptr })(unsafe.Pointer(current))
			if web := find(node.data); web != 0 {
				return web
			}
			current = node.next
		}
		return 0
	}
	return func(window *mygo.Window) {
		widget := find(window.NativeHandle())
		if widget == 0 {
			fmt.Fprintln(os.Stderr, "Native file drop: WebView unavailable")
			return
		}
		gtkFileDrops[widget] = &gtkFileDropState{window: window}
		api.connect(widget, "drag-motion", motionCallback, 0, 0, 0)
		api.connect(widget, "drag-drop", dropCallback, 0, 0, 0)
		api.connect(widget, "drag-data-received", dataCallback, 0, 0, 0)
		api.connect(widget, "destroy", destroyCallback, 0, 0, 0)
	}, nil
})

func installNativeFileDrop(window *mygo.Window) {
	install, err := installGTKFileDrop()
	if err != nil {
		fmt.Fprintln(os.Stderr, "Native file drop:", err)
		return
	}
	install(window)
}
