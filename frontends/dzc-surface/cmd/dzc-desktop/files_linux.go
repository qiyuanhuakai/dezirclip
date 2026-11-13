package main

import (
	"errors"
	"github.com/ebitengine/purego"
	"github.com/egoist/mygo"
	"net/url"
	"strings"
	"sync"
	"unsafe"
)

type gtkFileDragAPI struct {
	targetNew                                  func(uintptr, uint32) uintptr
	targetURIs                                 func(uintptr, uint32)
	targetFree                                 func(uintptr)
	connect                                    func(uintptr, string, uintptr, uintptr, uintptr, uint32) uint64
	disconnect                                 func(uintptr, uint64)
	begin                                      func(uintptr, uintptr, uint32, int32, uintptr, int32, int32) uintptr
	selectionTarget                            func(uintptr) uintptr
	selectionSet                               func(uintptr, uintptr, int32, uintptr, int32)
	widgetWindow                               func(uintptr) uintptr
	pointer                                    func(uintptr, *int32, *int32, *uint32) uintptr
	dataCallback, endCallback, destroyCallback uintptr
}
type gtkFileDragState struct {
	window   uintptr
	handlers []uint64
	bytes    []byte
	done     func(error)
}

// GTK owns all access, including completion and signal disconnect.
var activeFileDrag *gtkFileDragState
var fileDragGTK = sync.OnceValues(func() (*gtkFileDragAPI, error) {
	gtk, err := purego.Dlopen("libgtk-3.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	object, err := purego.Dlopen("libgobject-2.0.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	gdk, err := purego.Dlopen("libgdk-3.so.0", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	api := &gtkFileDragAPI{}
	purego.RegisterLibFunc(&api.targetNew, gtk, "gtk_target_list_new")
	purego.RegisterLibFunc(&api.targetURIs, gtk, "gtk_target_list_add_uri_targets")
	purego.RegisterLibFunc(&api.targetFree, gtk, "gtk_target_list_unref")
	purego.RegisterLibFunc(&api.connect, object, "g_signal_connect_data")
	purego.RegisterLibFunc(&api.disconnect, object, "g_signal_handler_disconnect")
	purego.RegisterLibFunc(&api.begin, gtk, "gtk_drag_begin_with_coordinates")
	purego.RegisterLibFunc(&api.selectionTarget, gtk, "gtk_selection_data_get_target")
	purego.RegisterLibFunc(&api.selectionSet, gtk, "gtk_selection_data_set")
	purego.RegisterLibFunc(&api.widgetWindow, gtk, "gtk_widget_get_window")
	purego.RegisterLibFunc(&api.pointer, gdk, "gdk_window_get_pointer")
	finish := func(widget uintptr, err error) {
		state := activeFileDrag
		if state == nil || state.window != widget {
			return
		}
		activeFileDrag = nil
		for _, id := range state.handlers {
			api.disconnect(widget, id)
		}
		state.done(err)
	}
	api.dataCallback = purego.NewCallback(func(widget, context, selection, info, timestamp, data uintptr) {
		state := activeFileDrag
		if state == nil || state.window != widget || len(state.bytes) == 0 {
			return
		}
		api.selectionSet(selection, api.selectionTarget(selection), 8, uintptr(unsafe.Pointer(&state.bytes[0])), int32(len(state.bytes)))
	})
	api.endCallback = purego.NewCallback(func(widget, context, data uintptr) { finish(widget, nil) })
	api.destroyCallback = purego.NewCallback(func(widget, data uintptr) { finish(widget, errors.New("FileDragWindowClosed")) })
	return api, nil
})

func startFileDrag(window *mygo.Window, paths []string, done func(error)) {
	api, err := fileDragGTK()
	if err != nil {
		done(err)
		return
	}
	if activeFileDrag != nil {
		done(errors.New("FileDragBusy"))
		return
	}
	widget := window.NativeHandle()
	native := api.widgetWindow(widget)
	var x, y int32
	var mask uint32
	if native != 0 {
		api.pointer(native, &x, &y, &mask)
	}
	if native == 0 || mask&(1<<8) == 0 || !window.IsVisible() {
		done(nil)
		return
	}
	uris := make([]string, 0, len(paths))
	for _, path := range paths {
		uris = append(uris, (&url.URL{Scheme: "file", Path: path}).String())
	}
	state := &gtkFileDragState{window: widget, bytes: []byte(strings.Join(uris, "\r\n") + "\r\n"), done: done}
	activeFileDrag = state
	state.handlers = []uint64{api.connect(widget, "drag-data-get", api.dataCallback, 0, 0, 0),
		api.connect(widget, "drag-end", api.endCallback, 0, 0, 0), api.connect(widget, "destroy", api.destroyCallback, 0, 0, 0)}
	targets := api.targetNew(0, 0)
	defer api.targetFree(targets)
	api.targetURIs(targets, 0)
	if api.begin(widget, targets, 2, 1, 0, -1, -1) == 0 && activeFileDrag == state {
		activeFileDrag = nil
		for _, id := range state.handlers {
			api.disconnect(widget, id)
		}
		done(errors.New("FileDragUnavailable"))
	}
}
