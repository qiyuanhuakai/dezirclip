package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
)

type desktopDiagnostics struct {
	mu                sync.Mutex
	path, role        string
	disconnected      bool
	statusUnavailable bool
}

func newDesktopDiagnostics(owner *client.Client, role string) *desktopDiagnostics {
	database, err := owner.DesktopDatabase()
	if err != nil {
		return nil
	}
	return &desktopDiagnostics{path: filepath.Join(filepath.Dir(database), "desktop-lifecycle-"+role+".log"), role: role}
}

func diagnosticError(err error) string {
	if err == nil {
		return ""
	}
	var remote *client.RemoteError
	if errors.As(err, &remote) {
		switch string(remote.Code) {
		case "busy", "unavailable", "privacy_blocked", "privacy_epoch_changed", "permission_denied", "session_expired":
			return "remote-" + string(remote.Code)
		default:
			return "remote"
		}
	}
	if errors.Is(err, client.ErrProtocol) {
		return "protocol"
	}
	if errors.Is(err, context.Canceled) {
		return "cancelled"
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return "deadline"
	}
	if errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) {
		return "eof"
	}
	var timeout net.Error
	if errors.As(err, &timeout) && timeout.Timeout() {
		return "transport-timeout"
	}
	if eventTransportFailure(err) {
		return "transport"
	}
	return "other"
}

// Only fixed lifecycle labels and error categories are stored. Never include
// error text, request arguments, clipboard data or foreground window titles.
func (d *desktopDiagnostics) record(event string, exitCode int, err error) {
	if d == nil {
		return
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	d.write(event, exitCode, err, 0)
}

func (d *desktopDiagnostics) childExited(pid, exitCode int, err error) {
	if d == nil {
		return
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	d.write("main-exited", exitCode, err, pid)
}

func (d *desktopDiagnostics) write(event string, exitCode int, err error, childPID int) {
	if info, err := os.Stat(d.path); err == nil && info.Size() > 64*1024 {
		_ = os.Remove(d.path + ".previous")
		_ = os.Rename(d.path, d.path+".previous")
	}
	file, openErr := os.OpenFile(d.path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
	if openErr != nil {
		return
	}
	defer file.Close()
	_ = json.NewEncoder(file).Encode(struct {
		Time     int64  `json:"time_ms"`
		PID      int    `json:"pid"`
		ChildPID int    `json:"child_pid,omitempty"`
		Role     string `json:"role"`
		Event    string `json:"event"`
		ExitCode int    `json:"exit_code"`
		Error    string `json:"error,omitempty"`
		Version  string `json:"version"`
		Revision string `json:"revision"`
	}{time.Now().UnixMilli(), os.Getpid(), childPID, d.role, event, exitCode, diagnosticError(err), buildinfo.Version, buildinfo.SourceRevision})
}

func (d *desktopDiagnostics) eventRead(err error) {
	if d == nil {
		return
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	if (err != nil) == d.disconnected {
		return
	}
	d.disconnected = err != nil
	if err != nil {
		d.write("events-unavailable", 0, err, 0)
	} else {
		d.write("events-recovered", 0, nil, 0)
	}
}

func (d *desktopDiagnostics) statusRead(err error) {
	if d == nil {
		return
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	if (err != nil) == d.statusUnavailable {
		return
	}
	d.statusUnavailable = err != nil
	if err != nil {
		d.write("privacy-status-unavailable", 0, err, 0)
	} else {
		d.write("privacy-status-recovered", 0, nil, 0)
	}
}
