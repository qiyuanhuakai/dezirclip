package main

import (
	"log"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

// Let Windows age idle pages during visibility changes. The hidden main
// window can trim once the UI and its history session have wound down.
const retainedMainTrimDelay = 10 * time.Second

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

func trimRetainedMainWorkingSet() {
	trim := windows.NewLazySystemDLL("psapi.dll").NewProc("EmptyWorkingSet")
	if trim.Find() == nil {
		trim.Call(uintptr(windows.CurrentProcess()))
	}
}
