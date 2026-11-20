package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"sync"
	"time"

	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

// A reply with release notes may be larger than the default scanner buffer.
const residentFrameLimit = 2 * 1024 * 1024

// The resident owns the tray and native selector. Only its main child has these pipes.
type residentRequest struct {
	Update     *mygoInstallRequest `json:"update,omitempty"`
	Appearance *theme.Appearance   `json:"appearance,omitempty"`
	Preview    *pinPreviewRequest  `json:"preview,omitempty"`
	ID         uint64              `json:"request_id"`
	Epoch      uint64              `json:"epoch"`
	Operation  string              `json:"operation"`
	Directory  string              `json:"directory,omitempty"`
	Panel      string              `json:"panel,omitempty"`
	Hotkey     string              `json:"hotkey,omitempty"`
	Disabled   bool                `json:"disabled,omitempty"`
	Full       bool                `json:"full,omitempty"`
	Visible    bool                `json:"visible,omitempty"`

	UpdateCheck   *nativeUpdateCheck `json:"update_check,omitempty"`
	UpdateChannel string             `json:"update_channel,omitempty"`
}
type residentReply struct {
	ID        uint64 `json:"request_id"`
	Reconnect bool   `json:"reconnect,omitempty"`
	Success   bool   `json:"success"`
	Error     string `json:"error,omitempty"`

	UpdateCheck *nativeUpdateCheck `json:"update_check,omitempty"`
}
type residentBridge struct {
	output     io.Writer
	grantFocus func() error
	mu         sync.Mutex
	id         uint64
	pending    chan residentReply
	operation  string
	finished   chan struct{}
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
	return c.exchange(ctx, request, deadline, nil)
}

func (c *residentBridge) exchange(ctx context.Context, request residentRequest, deadline time.Duration, response *residentReply) error {
	timeout, cancel := context.WithCancel(ctx)
	if deadline > 0 {
		cancel()
		timeout, cancel = context.WithTimeout(ctx, deadline)
	}
	defer cancel()
	for {
		c.mu.Lock()
		if err := timeout.Err(); err != nil {
			c.mu.Unlock()
			return err
		}
		if c.pending == nil {
			break
		}
		if !residentBackgroundRequest(c.operation) && !residentBackgroundRequest(request.Operation) {
			c.mu.Unlock()
			if request.Operation == "capture" {
				return errors.New("ScreenshotBusy")
			}
			return errors.New("ResidentBusy")
		}
		// Background synchronization waits for the active request to finish.
		// Wait before sending anything; a timed-out action is never replayed.
		finished := c.finished
		c.mu.Unlock()
		select {
		case <-finished:
		case <-timeout.Done():
			return timeout.Err()
		}
	}
	c.id++
	done := make(chan residentReply, 1)
	c.pending = done
	c.operation = request.Operation
	c.finished = make(chan struct{})
	request.ID = c.id
	c.mu.Unlock()
	defer func() {
		c.mu.Lock()
		c.pending = nil
		close(c.finished)
		c.operation = ""
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
	select {
	case reply := <-done:
		if request.Operation == "move-profile" && reply.Success && reply.Reconnect {
			return errReconnect
		}
		if !reply.Success {
			if request.Operation == "restart-admin" {
				if reply.Error != "" {
					return errors.New(reply.Error)
				}
				return errors.New("ElevationUnavailable")
			}
			if request.Operation == "recording" || request.Operation == "native-panel" || request.Operation == "test-hotkey" || request.Operation == "main-hotkeys" || request.Operation == "panel-hotkey" || request.Operation == "search-hotkey" || request.Operation == "paste-hotkey" {
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
			if request.Operation == "install-update" && reply.Error != "" {
				return errors.New(reply.Error)
			}
			return errors.New("TrayUnavailable")
		}
		if response != nil {
			*response = reply
		}
		return nil
	case <-timeout.Done():
		return timeout.Err()
	}
}

func residentBackgroundRequest(operation string) bool {
	return operation == "appearance" || operation == "update-check"
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
