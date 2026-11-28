package main

import (
	"log"
	"os"
	"time"

	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/desktop/application"
)

func desktopTimingEnabled() bool { return os.Getenv("DZC_DESKTOP_TIMING") == "1" }

// Opt-in diagnostics contain stage durations and byte counts, never content or resource paths.
func desktopTiming(stage string, started time.Time) {
	if desktopTimingEnabled() {
		log.Printf("Desktop timing: %s %dms", stage, time.Since(started).Milliseconds())
	}
}

func desktopSnapshotTiming(sample application.SnapshotMeasurement) {
	log.Printf("Desktop snapshot: %s bytes=%d elapsed=%dms snapshot_calls=%d connect_us=%d descriptor_us=%d transfer_us=%d decode_us=%d complete=%t",
		sample.Method, sample.Bytes, sample.Elapsed.Milliseconds(), 1+sample.Metrics.Chunks, sample.Connect.Microseconds(),
		sample.Descriptor.Microseconds(), sample.Metrics.Transfer.Microseconds(), sample.Metrics.Decode.Microseconds(), sample.Complete)
}
