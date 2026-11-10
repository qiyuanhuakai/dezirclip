package main

import (
	"log"
	"os"
	"time"
)

// Opt-in diagnostics contain stage durations only, never history or resource paths.
func desktopTiming(stage string, started time.Time) {
	if os.Getenv("DZC_DESKTOP_TIMING") == "1" {
		log.Printf("Desktop timing: %s %dms", stage, time.Since(started).Milliseconds())
	}
}
