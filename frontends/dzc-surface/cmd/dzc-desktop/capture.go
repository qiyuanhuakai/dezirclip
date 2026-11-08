package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"sync"
	"time"
)

// The resident owns the tray and native selector. Only its original WebView child has these pipes.
type residentRequest struct {
	Preview   *pinPreviewRequest `json:"preview,omitempty"`
	ID        uint64             `json:"request_id"`
	Epoch     uint64             `json:"epoch"`
	Operation string             `json:"operation"`
	Directory string             `json:"directory,omitempty"`
	Panel     string             `json:"panel,omitempty"`
	Hotkey    string             `json:"hotkey,omitempty"`
	Disabled  bool               `json:"disabled,omitempty"`
	Full      bool               `json:"full,omitempty"`
	Visible   bool               `json:"visible,omitempty"`
}
type residentReply struct {
	ID        uint64 `json:"request_id"`
	Reconnect bool   `json:"reconnect,omitempty"`
	Success   bool   `json:"success"`
	Error     string `json:"error,omitempty"`
}
type residentBridge struct {
	output     io.Writer
	grantFocus func() error
	mu         sync.Mutex
	id         uint64
	pending    chan residentReply
}

var errReconnect = errors.New("ProfileReconnectRequired")

func (s *DesktopShell) MoveProfile(ctx context.Context, directory string, epoch uint64) (bool, error) {
	err := s.resident.send(ctx, residentRequest{Operation: "move-profile", Directory: directory, Epoch: epoch}, 15*time.Minute)
	if errors.Is(err, errReconnect) {
		return true, nil
	}
	return false, err
}

func (s *DesktopShell) RelaunchProfile(ctx context.Context) error {
	return s.resident.send(ctx, residentRequest{Operation: "relaunch-profile"}, 60*time.Second)
}

func (s *DesktopShell) Capture(ctx context.Context, full bool, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "capture", Full: full, Epoch: epoch}, 115*time.Second)
}

func (s *DesktopShell) SetWebviewGpuDisabled(ctx context.Context, disabled bool, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "webview-gpu", Disabled: disabled, Epoch: epoch}, 10*time.Second)
}

func (s *DesktopShell) SetTrayVisible(ctx context.Context, visible bool, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "tray", Visible: visible, Epoch: epoch}, 10*time.Second)
}

func (s *DesktopShell) SetNativePanelEnabled(ctx context.Context, panel string, enabled bool, epoch uint64) error {
	if panel != "quick-paste" && panel != "screenshot" {
		return errors.New("InvalidNativePanel")
	}
	return s.resident.send(ctx, residentRequest{Operation: "native-panel", Panel: panel, Disabled: !enabled, Epoch: epoch}, 15*time.Second)
}

func (s *DesktopShell) TestHotkeyAvailable(ctx context.Context, hotkey string) (bool, error) {
	err := s.resident.send(ctx, residentRequest{Operation: "test-hotkey", Hotkey: hotkey}, 6*time.Second)
	return err == nil, err
}

func (c *residentBridge) send(ctx context.Context, request residentRequest, deadline time.Duration) error {
	c.mu.Lock()
	if c.pending != nil {
		c.mu.Unlock()
		if request.Operation == "capture" {
			return errors.New("ScreenshotBusy")
		}
		return errors.New("ResidentBusy")
	}
	c.id++
	done := make(chan residentReply, 1)
	c.pending = done
	request.ID = c.id
	c.mu.Unlock()
	defer func() {
		c.mu.Lock()
		c.pending = nil
		c.mu.Unlock()
	}()
	if request.Operation == "capture" && !request.Full && c.grantFocus != nil {
		if err := c.grantFocus(); err != nil {
			return err
		}
	}
	if err := json.NewEncoder(c.output).Encode(request); err != nil {
		return err
	}
	timeout, cancel := context.WithTimeout(ctx, deadline)
	defer cancel()
	select {
	case reply := <-done:
		if request.Operation == "move-profile" && reply.Success && reply.Reconnect {
			return errReconnect
		}
		if !reply.Success {
			if request.Operation == "recording" || request.Operation == "native-panel" || request.Operation == "test-hotkey" || request.Operation == "main-hotkeys" || request.Operation == "panel-hotkey" || request.Operation == "search-hotkey" {
				if reply.Error != "" {
					return errors.New(reply.Error)
				}
				return errors.New("NativeShortcutUnavailable")
			}
			if request.Operation == "move-profile" || request.Operation == "relaunch-profile" {
				if reply.Error != "" {
					return errors.New(reply.Error)
				}
				return errors.New("ProfileMoveUnavailable")
			}
			if request.Operation == "capture" {
				return errors.New("ScreenshotUnavailable")
			}
			if request.Operation == "pin-preview" {
				return errors.New("PreviewUnavailable")
			}
			if request.Operation == "webview-gpu" {
				return errors.New("WebViewRebuildUnavailable")
			}
			return errors.New("TrayUnavailable")
		}
		return nil
	case <-timeout.Done():
		return timeout.Err()
	}
}

func (c *residentBridge) accept(line string) {
	var reply residentReply
	if json.Unmarshal([]byte(line), &reply) != nil {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.pending != nil && c.id == reply.ID {
		select {
		case c.pending <- reply:
		default:
		}
	}
}
