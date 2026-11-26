package main

import (
	"log"
	"runtime/debug"
	"runtime/metrics"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

// MyGo releases its surface caches after two seconds without frames.
const hiddenWorkingSetTrimDelay = 3 * time.Second

const (
	processMemoryPriority = 0 // PROCESS_INFORMATION_CLASS.ProcessMemoryPriority
	memoryPriorityLow     = uint32(2)
	memoryPriorityNormal  = uint32(5)
)

// Retain the foreground tuning so each visible window uses the normal GC policy.
var backgroundGC struct {
	active  bool
	percent int
	limit   int64
}

func setProcessMemoryBackground(background bool) {
	if !background && backgroundGC.active {
		debug.SetMemoryLimit(backgroundGC.limit)
		debug.SetGCPercent(backgroundGC.percent)
		backgroundGC.active = false
	}
	process := windows.CurrentProcess()
	priority := memoryPriorityNormal
	if background {
		priority = memoryPriorityLow
	}
	set := windows.NewLazySystemDLL("kernel32.dll").NewProc("SetProcessInformation")
	if err := set.Find(); err != nil {
		return
	}
	if ok, _, err := set.Call(uintptr(process), processMemoryPriority, uintptr(unsafe.Pointer(&priority)), unsafe.Sizeof(priority)); ok == 0 {
		log.Printf("Process memory priority unavailable: %v", err)
	}
}

func trimHiddenWorkingSet() {
	debug.FreeOSMemory()
	if !backgroundGC.active {
		memory := [2]metrics.Sample{
			{Name: "/memory/classes/total:bytes"},
			{Name: "/memory/classes/heap/released:bytes"},
		}
		metrics.Read(memory[:])
		// With no visible UI, collection can wait for memory pressure. Leave
		// room for clipboard and shortcut work while the windows are hidden.
		const hiddenHeapHeadroom = 32 << 20
		limit := int64(memory[0].Value.Uint64()-memory[1].Value.Uint64()) + hiddenHeapHeadroom
		backgroundGC.limit = debug.SetMemoryLimit(-1)
		debug.SetMemoryLimit(min(backgroundGC.limit, limit))
		backgroundGC.percent = debug.SetGCPercent(-1)
		backgroundGC.active = true
	}
	trim := windows.NewLazySystemDLL("psapi.dll").NewProc("EmptyWorkingSet")
	if trim.Find() == nil {
		trim.Call(uintptr(windows.CurrentProcess()))
	}
}
