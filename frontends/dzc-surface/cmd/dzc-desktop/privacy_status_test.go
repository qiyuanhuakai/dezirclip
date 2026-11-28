package main

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestPrivacyStatusDiagnosticsAreIndependentAndRedacted(t *testing.T) {
	d := &desktopDiagnostics{path: filepath.Join(t.TempDir(), "desktop.log"), role: "main"}
	d.eventRead(nil)
	d.statusRead(nil)
	d.statusRead(&client.RemoteError{Code: protocol.WireErrorCodeBusy})
	d.statusRead(errors.New("sensitive content"))
	d.statusRead(nil)
	d.statusRead(nil)
	raw, err := os.ReadFile(d.path)
	if err != nil {
		t.Fatal(err)
	}
	log := string(raw)
	if strings.Count(log, "\n") != 2 || strings.Contains(log, "sensitive") || strings.Contains(log, "events-") ||
		!strings.Contains(log, "remote-busy") || !strings.Contains(log, "privacy-status-recovered") {
		t.Fatal(log)
	}
}
