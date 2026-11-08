package main

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type SystemSettings struct {
	owner   *client.Client
	mu      sync.Mutex
	pending *systemChange
}

type systemChange struct {
	host  systemSettingsConnection
	token string
	stop  chan struct{}
	done  chan struct{}
	once  sync.Once
}

type systemSettingsConnection interface {
	Call(context.Context, protocol.Method, any, string, any) error
	Close() error
}

func (c *systemChange) stopPolling() {
	c.once.Do(func() { close(c.stop) })
	<-c.done
}

func (s *SystemSettings) close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.pending != nil {
		s.pending.stopPolling()
		s.pending.host.Close()
		s.pending = nil
	}
}

func (s *SystemSettings) WinVStatus(ctx context.Context) (bool, error) {
	var result protocol.WindowsSettingResult
	err := s.owner.DesktopCall(ctx, protocol.MethodDesktopSystemWinVGet, struct{}{}, "WindowsSettingResult", &result)
	return result.Value, err
}

func (s *SystemSettings) IsAdmin(ctx context.Context) (bool, error) {
	var result protocol.WindowsSettingResult
	err := s.owner.DesktopCall(ctx, protocol.MethodDesktopSystemAdminGet, struct{}{}, "WindowsSettingResult", &result)
	return result.Value, err
}

func (s *SystemSettings) RestartExplorer(ctx context.Context) error {
	var result protocol.WindowsSettingResult
	return s.owner.DesktopCall(ctx, protocol.MethodDesktopSystemExplorerRestart, struct{}{}, "WindowsSettingResult", &result)
}

func (s *SystemSettings) SetClipboardHistory(ctx context.Context, enabled bool) error {
	var result protocol.WindowsSettingResult
	return s.owner.DesktopCall(ctx, protocol.MethodDesktopSystemClipboardHistorySet, protocol.WindowsSettingParams{Enabled: enabled}, "WindowsSettingResult", &result)
}

func (s *SystemSettings) BeginWinV(ctx context.Context, enabled bool) (protocol.WindowsSettingResult, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	var result protocol.WindowsSettingResult
	s.discardDisconnectedChange()
	if s.pending != nil {
		return result, errors.New("WindowsSettingsBusy")
	}
	host, err := s.owner.DesktopConnection(ctx)
	if err != nil {
		return result, err
	}
	if err = host.Call(ctx, protocol.MethodDesktopSystemWinVBegin, protocol.WindowsSettingParams{Enabled: enabled}, "WindowsSettingResult", &result); err != nil {
		host.Close()
		return result, err
	}
	change := &systemChange{host: host, token: result.Token, stop: make(chan struct{}), done: make(chan struct{})}
	s.pending = change
	go func() {
		defer close(change.done)
		ticker := time.NewTicker(10 * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-change.stop:
				return
			case <-ticker.C:
				ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
				var state protocol.RuntimeState
				err := host.Call(ctx, protocol.MethodHealth, struct{}{}, "RuntimeState", &state)
				cancel()
				if err != nil {
					host.Close()
					return
				}
			}
		}
	}()
	return result, nil
}

func (s *SystemSettings) FinishWinV(ctx context.Context, token string, rollback bool) (bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.discardDisconnectedChange()
	if s.pending == nil || s.pending.token != token {
		return false, errors.New("WindowsSettingsTokenUnavailable")
	}
	change := s.pending
	var result protocol.WindowsSettingResult
	err := change.host.Call(ctx, protocol.MethodDesktopSystemWinVFinish, protocol.WindowsSettingFinishParams{Token: token, Rollback: rollback}, "WindowsSettingResult", &result)
	if err == nil {
		change.stopPolling()
		change.host.Close()
		s.pending = nil
	} else {
		var remote *client.RemoteError
		if !errors.As(err, &remote) {
			change.stopPolling()
			change.host.Close()
			s.pending = nil
		}
	}
	return result.Value, err
}

// Called with mu held. The polling worker closes done after closing a failed
// connection; it never needs mu, so a foreground finish cannot deadlock it.
func (s *SystemSettings) discardDisconnectedChange() {
	if s.pending == nil {
		return
	}
	select {
	case <-s.pending.done:
		s.pending.host.Close()
		s.pending = nil
	default:
	}
}
