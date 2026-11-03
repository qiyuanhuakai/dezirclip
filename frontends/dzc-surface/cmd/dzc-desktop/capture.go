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
	ID        uint64 `json:"request_id"`
	Epoch     uint64 `json:"epoch"`
	Operation string `json:"operation"`
	Full      bool   `json:"full,omitempty"`
	Visible   bool   `json:"visible,omitempty"`
}
type residentReply struct {
	ID      uint64 `json:"request_id"`
	Success bool   `json:"success"`
}
type residentBridge struct {
	output     io.Writer
	grantFocus func() error
	mu         sync.Mutex
	id         uint64
	pending    chan bool
}

func (s *DesktopShell) Capture(ctx context.Context, full bool, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "capture", Full: full, Epoch: epoch}, 115*time.Second)
}

func (s *DesktopShell) SetTrayVisible(ctx context.Context, visible bool, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "tray", Visible: visible, Epoch: epoch}, 10*time.Second)
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
	done := make(chan bool, 1)
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
	case success := <-done:
		if !success {
			if request.Operation == "capture" {
				return errors.New("ScreenshotUnavailable")
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
		case c.pending <- reply.Success:
		default:
		}
	}
}
