package main

import (
	"log"
	"runtime/debug"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

// MyGo releases its surface caches after two seconds without frames.
const hiddenWorkingSetTrimDelay = 3 * time.Second

func setProcessMemoryBackground(background bool) {
	process := windows.CurrentProcess()
	priority := uint32(5)
	if background {
		priority = 1
	}
	set := windows.NewLazySystemDLL("kernel32.dll").NewProc("SetProcessInformation")
	if err := set.Find(); err != nil {
		return
	}
	if ok, _, err := set.Call(uintptr(process), 0, uintptr(unsafe.Pointer(&priority)), unsafe.Sizeof(priority)); ok == 0 {
		log.Printf("Process memory priority unavailable: %v", err)
	}
}

func trimHiddenWorkingSet() {
	debug.FreeOSMemory()
	trim := windows.NewLazySystemDLL("psapi.dll").NewProc("EmptyWorkingSet")
	if trim.Find() == nil {
		trim.Call(uintptr(windows.CurrentProcess()))
	}
}
