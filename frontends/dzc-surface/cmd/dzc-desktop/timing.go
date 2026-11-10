package main

import (
	"log"
	"os"
	"time"
)

// Opt-in diagnostics contain stage durations and byte counts, never content or resource paths.
func desktopTiming(stage string, started time.Time) {
	if os.Getenv("DZC_DESKTOP_TIMING") == "1" {
		log.Printf("Desktop timing: %s %dms", stage, time.Since(started).Milliseconds())
	}
}

func desktopSnapshotTiming(method string, bytes uint32, started time.Time) {
	if os.Getenv("DZC_DESKTOP_TIMING") == "1" {
		log.Printf("Desktop snapshot: %s bytes=%d elapsed=%dms", method, bytes, time.Since(started).Milliseconds())
	}
}
