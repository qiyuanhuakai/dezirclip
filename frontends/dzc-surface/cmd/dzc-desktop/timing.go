package main

import (
	"log"
	"os"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func desktopTimingEnabled() bool { return os.Getenv("DZC_DESKTOP_TIMING") == "1" }

// Opt-in diagnostics contain stage durations and byte counts, never content or resource paths.
func desktopTiming(stage string, started time.Time) {
	if desktopTimingEnabled() {
		log.Printf("Desktop timing: %s %dms", stage, time.Since(started).Milliseconds())
	}
}

func desktopSnapshotTiming(method protocol.Method, bytes uint32, started, connected, described time.Time, metrics client.SnapshotReadMetrics, complete bool) {
	log.Printf("Desktop snapshot: %s bytes=%d elapsed=%dms snapshot_calls=%d connect_us=%d descriptor_us=%d transfer_us=%d decode_us=%d complete=%t",
		method, bytes, time.Since(started).Milliseconds(), 1+metrics.Chunks, connected.Sub(started).Microseconds(),
		described.Sub(connected).Microseconds(), metrics.Transfer.Microseconds(), metrics.Decode.Microseconds(), complete)
}
