package main

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func (s *DesktopShell) SetMainHotkeys(ctx context.Context, hotkeys string, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "main-hotkeys", Hotkey: hotkeys, Epoch: epoch}, 10*time.Second)
}

func (s *DesktopShell) ResetSettings(ctx context.Context, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "reset-settings", Epoch: epoch}, 15*time.Second)
}

func resetSettings(owner *client.Client, epoch uint64) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	var result protocol.OperationResult
	err := owner.DesktopCall(ctx, protocol.MethodDesktopSettingsReset, protocol.LibraryStateParams{
		ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch,
	}, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

func (s *DesktopShell) SetSearchHotkey(ctx context.Context, hotkey string, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "search-hotkey", Hotkey: hotkey, Epoch: epoch}, 10*time.Second)
}

func (s *DesktopShell) SetPasteHotkey(ctx context.Context, key, hotkey string, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "paste-hotkey", Panel: key, Hotkey: hotkey, Epoch: epoch}, 10*time.Second)
}

func persistActionHotkey(owner *client.Client, key, hotkey string, epoch uint64) error {
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
	defer cancel()
	var result protocol.OperationResult
	err := owner.DesktopCall(ctx, protocol.MethodDesktopHotkeyActionSet, protocol.ActionHotkeyParams{
		ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Key: key, Hotkey: hotkey,
	}, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

func (s *DesktopShell) SetPanelHotkey(ctx context.Context, key, hotkey string, epoch uint64) error {
	return s.resident.send(ctx, residentRequest{Operation: "panel-hotkey", Panel: key, Hotkey: hotkey, Epoch: epoch}, 15*time.Second)
}

func persistPanelHotkey(owner *client.Client, key, hotkey string, epoch uint64) error {
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
	defer cancel()
	var result protocol.OperationResult
	err := owner.DesktopCall(ctx, protocol.MethodDesktopHotkeyPanelSet, protocol.PanelHotkeyParams{
		ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Key: key, Hotkey: hotkey,
	}, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

// Keep old registrations until both new native claims and persistence succeed.
func changeMainHotkeys(registered map[string]bool, raw string, register func(string) error, unregister func(string), save func() error) error {
	wanted := make(map[string]bool)
	for _, key := range strings.FieldsFunc(raw, func(r rune) bool { return r == '\r' || r == '\n' }) {
		key = strings.TrimSpace(key)
		if key != "" {
			wanted[key] = true
		}
	}
	if len(raw) > 2064 || len(wanted) > 16 {
		return errors.New("InvalidShortcut")
	}
	added := make([]string, 0, len(wanted))
	rollback := func() {
		for _, key := range added {
			unregister(key)
		}
	}
	for key := range wanted {
		if !registered[key] {
			if err := register(key); err != nil {
				rollback()
				return err
			}
			added = append(added, key)
		}
	}
	if err := save(); err != nil {
		rollback()
		return err
	}
	for key := range registered {
		if !wanted[key] {
			unregister(key)
			delete(registered, key)
		}
	}
	for _, key := range added {
		registered[key] = true
	}
	return nil
}

func persistMainHotkeys(owner *client.Client, hotkeys string, epoch uint64) error {
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
	defer cancel()
	var result protocol.OperationResult
	err := owner.DesktopCall(ctx, protocol.MethodDesktopHotkeyMainSet, protocol.MainHotkeyParams{
		ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Hotkeys: hotkeys,
	}, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

// Run on the resident UI thread, which owns every desktop global shortcut.
func testNativeHotkey(hotkey string) error {
	if hotkey == "" {
		return nil
	}
	if len(hotkey) > 128 || strings.ContainsAny(hotkey, "\r\n\x00") {
		return errors.New("InvalidShortcut")
	}
	// Register rejects an existing local claim without changing its callback.
	// Only a claim created by this probe may be released here.
	if err := mygo.GlobalShortcut.Register(hotkey, func() {}); err != nil {
		return err
	}
	mygo.GlobalShortcut.Unregister(hotkey)
	return nil
}
