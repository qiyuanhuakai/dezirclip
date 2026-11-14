package main

import (
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDesktopDiagnosticsRedactsErrorsAndRecordsTransitionsOnly(t *testing.T) {
	d := &desktopDiagnostics{path: filepath.Join(t.TempDir(), "desktop.log"), role: "main"}
	d.eventRead(nil)
	d.eventRead(errors.New("private clipboard path and payload"))
	d.eventRead(io.EOF)
	d.eventRead(nil)
	d.eventRead(nil)
	raw, err := os.ReadFile(d.path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(raw)
	if strings.Count(text, "\n") != 2 || strings.Contains(text, "private") ||
		!strings.Contains(text, "events-unavailable") || !strings.Contains(text, "events-recovered") {
		t.Fatalf("invalid redacted lifecycle: %s", text)
	}
	if err := os.WriteFile(d.path, []byte(strings.Repeat("x", 65*1024)), 0600); err != nil {
		t.Fatal(err)
	}
	d.record("main-exited", -1073741819, nil)
	info, err := os.Stat(d.path + ".previous")
	if err != nil || info.Size() != 65*1024 {
		t.Fatalf("rotation: %v", err)
	}
	var absent *desktopDiagnostics
	absent.record("start", 0, nil)
	absent.eventRead(io.EOF)
}
