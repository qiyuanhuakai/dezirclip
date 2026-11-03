package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"sync"
	"time"
)

// The WebView and resident share only inherited pipes. The resident owns the one native selector.
type captureRequest struct {
	ID    uint64 `json:"capture"`
	Epoch uint64 `json:"epoch"`
	Full  bool   `json:"full"`
}
type captureReply struct {
	ID      uint64 `json:"capture"`
	Success bool   `json:"success"`
}
type residentCapture struct {
	output     io.Writer
	grantFocus func() error
	mu         sync.Mutex
	id         uint64
	pending    chan bool
}

func (s *DesktopShell) Capture(ctx context.Context, full bool, epoch uint64) error {
	c := &s.capture
	c.mu.Lock()
	if c.pending != nil {
		c.mu.Unlock()
		return errors.New("ScreenshotBusy")
	}
	c.id++
	done := make(chan bool, 1)
	c.pending = done
	request := captureRequest{ID: c.id, Full: full, Epoch: epoch}
	c.mu.Unlock()
	defer func() {
		c.mu.Lock()
		c.pending = nil
		c.mu.Unlock()
	}()
	if !full && c.grantFocus != nil {
		if err := c.grantFocus(); err != nil {
			return err
		}
	}
	if err := json.NewEncoder(c.output).Encode(request); err != nil {
		return err
	}
	timeout, cancel := context.WithTimeout(ctx, 115*time.Second)
	defer cancel()
	select {
	case success := <-done:
		if !success {
			return errors.New("ScreenshotUnavailable")
		}
		return nil
	case <-timeout.Done():
		return timeout.Err()
	}
}

func (c *residentCapture) accept(line string) {
	var reply captureReply
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
