package main

import (
	"fmt"
	"github.com/egoist/mygo"
	"golang.org/x/sys/windows"
	"os"
	"runtime"
	"syscall"
	"unsafe"
)

var fileDragShell = windows.NewLazySystemDLL("shell32.dll")
var createFileData = fileDragShell.NewProc("SHCreateDataObject")
var dragFileData = fileDragShell.NewProc("SHDoDragDrop")
var fileDragKernel = windows.NewLazySystemDLL("kernel32.dll")
var dragAlloc = fileDragKernel.NewProc("GlobalAlloc")
var dragLock = fileDragKernel.NewProc("GlobalLock")
var dragUnlock = fileDragKernel.NewProc("GlobalUnlock")
var dragFree = fileDragKernel.NewProc("GlobalFree")
var dragKey = windows.NewLazySystemDLL("user32.dll").NewProc("GetAsyncKeyState")

type dragFormat struct {
	format uint16
	target uintptr
	aspect uint32
	index  int32
	medium uint32
}
type dragMedium struct {
	kind    uint32
	data    uintptr
	release uintptr
}

// Called on MyGo's STA UI thread; Shell owns the nested OLE drag loop.
func startFileDrag(window *mygo.Window, paths []string, done func(error)) {
	done(dragFilesWindows(window, paths))
}
func dragFilesWindows(window *mygo.Window, paths []string) error {
	pressed, _, _ := dragKey.Call(1)
	if pressed&0x8000 == 0 || !window.IsVisible() {
		return nil
	}
	var names []uint16
	for _, path := range paths {
		value, err := windows.UTF16FromString(path)
		if err != nil {
			return err
		}
		names = append(names, value...)
	}
	names = append(names, 0)
	iid := windows.GUID{Data1: 0x10e, Data4: [8]byte{0xc0, 0, 0, 0, 0, 0, 0, 0x46}}
	var data uintptr
	hr, _, _ := createFileData.Call(0, 0, 0, 0, uintptr(unsafe.Pointer(&iid)), uintptr(unsafe.Pointer(&data)))
	if int32(hr) < 0 {
		return fmt.Errorf("FileDragData: 0x%x", hr)
	}
	vtable := *(*[12]uintptr)(unsafe.Pointer(*(*uintptr)(unsafe.Pointer(data))))
	defer syscall.SyscallN(vtable[2], data)
	memory, _, _ := dragAlloc.Call(2, uintptr(20+2*len(names)))
	if memory == 0 {
		return fmt.Errorf("FileDragAllocationFailed")
	}
	address, _, _ := dragLock.Call(memory)
	if address == 0 {
		dragFree.Call(memory)
		return fmt.Errorf("FileDragAllocationFailed")
	}
	buffer := unsafe.Slice((*byte)(unsafe.Pointer(address)), 20+2*len(names))
	clear(buffer)
	buffer[0] = 20
	buffer[16] = 1
	copy(unsafe.Slice((*uint16)(unsafe.Pointer(address+20)), len(names)), names)
	dragUnlock.Call(memory)
	format := dragFormat{format: 15, aspect: 1, index: -1, medium: 1}
	medium := dragMedium{kind: 1, data: memory}
	hr, _, _ = syscall.SyscallN(vtable[7], data, uintptr(unsafe.Pointer(&format)), uintptr(unsafe.Pointer(&medium)), 1)
	if int32(hr) < 0 {
		dragFree.Call(memory)
		return fmt.Errorf("FileDragSetData: 0x%x", hr)
	}
	var effect uint32
	hr, _, _ = dragFileData.Call(window.NativeHandle(), data, 0, 1, uintptr(unsafe.Pointer(&effect)))
	fmt.Fprintf(os.Stderr, "Native file drag completed: result=0x%x effect=%d\n", hr, effect)
	runtime.KeepAlive(names)
	if int32(hr) < 0 {
		return fmt.Errorf("FileDrag: 0x%x", hr)
	}
	return nil
}
