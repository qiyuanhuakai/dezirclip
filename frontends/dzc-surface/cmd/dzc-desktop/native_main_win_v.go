package main

import (
	"context"
	"errors"
	"log"
	"runtime"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

const winVSetting = "app.use_win_v_shortcut"
const winVBinding = "Super+V"

type nativeWinVState struct {
	enabled, known, loading, failed bool
	confirm, changing               bool
}

func (n *nativeMain) canRemoveMainShortcut(binding string) bool {
	if runtime.GOOS != "windows" || binding != winVBinding {
		return true
	}
	if n.winV.known {
		return !n.winV.enabled
	}
	return n.preferences[winVSetting] != "true"
}

func (n *nativeMain) winVSettings(c *ui.Context, kit uikit.Kit) {
	if runtime.GOOS != "windows" || n.system == nil {
		return
	}
	state := &n.winV
	if !state.known && !state.failed {
		n.loadWinVStatus()
	}
	enabled := state.enabled
	if !state.known {
		enabled = n.preferences[winVSetting] == "true"
	}
	ui.Column(c).ColumnSpan(-1).Gap(6).Children(func() {
		label := n.text("use_win_v_shortcut")
		if kit.SettingsToggleField(c, &enabled, label, n.settingsHelpLabel(label), n.text("use_win_v_shortcut_hint"), !state.known || !n.shortcutAvailable()) {
			state.confirm = true
		}
		if state.failed && kit.Button(c, uikit.ButtonOptions{Label: n.text("common_retry"),
			Disabled: state.loading || !n.shortcutAvailable()}).Clicked() {
			n.loadWinVStatus()
		}
	})
}

func (n *nativeMain) loadWinVStatus() {
	if n.session == nil || n.history.state.ServiceInstanceId == "" || n.winV.loading || n.winV.changing {
		return
	}
	n.winV.loading, n.winV.failed = true, false
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		enabled, err := n.system.WinVStatus(ctx)
		n.update(session, func() {
			n.winV.enabled, n.winV.known, n.winV.loading, n.winV.failed = enabled, err == nil, false, err != nil
		})
	}()
}

func (n *nativeMain) confirmWinV() {
	if n.system == nil || !n.winV.known || !n.shortcutAvailable() {
		return
	}
	enabled := !n.winV.enabled
	oldKeys := n.preferences[mainShortcutSetting]
	oldPreference := strconv.FormatBool(n.preferences[winVSetting] == "true")
	epoch := n.history.state.PrivacyEpoch
	n.winV.changing, n.mutating, n.notice = true, true, ""
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 50*time.Second)
		defer cancel()
		err := changeNativeWinV(ctx, n.system, enabled, oldKeys, oldPreference,
			func(ctx context.Context, value string) error { return n.shell.SetMainHotkeys(ctx, value, epoch) },
			func(ctx context.Context, value string) error {
				return n.library.SetSetting(ctx, winVSetting, value, epoch)
			})
		if err != nil {
			n.system.close()
		}
		n.window.Update(func() {
			n.winV = nativeWinVState{}
			if n.window.IsDestroyed() {
				return
			}
			n.mutating = false
			if n.session != nil {
				n.prepare(n.resume)
			}
			if err != nil {
				log.Printf("Win+V change failed: %v", err)
				n.notice = n.text("native_setting_save_failed")
			}
		})
	}()
}

func withWinVBinding(raw string, enabled bool) string {
	bindings := strings.FieldsFunc(raw, func(r rune) bool { return r == '\r' || r == '\n' })
	kept := bindings[:0]
	contains := false
	for _, binding := range bindings {
		if mainShortcutName(binding) == winVBinding {
			contains = true
		} else {
			kept = append(kept, binding)
		}
	}
	if enabled == contains {
		return raw
	}
	if enabled {
		kept = append(kept, winVBinding)
	}
	return strings.Join(kept, "\n")
}

type winVSystem interface {
	BeginWinV(context.Context, bool) (protocol.WindowsSettingResult, error)
	FinishWinV(context.Context, string, bool) (bool, error)
	RestartExplorer(context.Context) error
}

func changeNativeWinV(ctx context.Context, system winVSystem, enabled bool, oldKeys, oldPreference string,
	setHotkeys, setPreference func(context.Context, string) error) (err error) {
	receipt, err := system.BeginWinV(ctx, enabled)
	if err != nil {
		return err
	}
	if receipt.Token == "" {
		return client.ErrProtocol
	}
	newKeys := withWinVBinding(oldKeys, enabled)
	var keysAttempted, restartAttempted, preferenceAttempted bool
	defer func() {
		if err == nil {
			return
		}
		cleanup, cancel := context.WithTimeout(context.Background(), 45*time.Second)
		defer cancel()
		// Release a newly claimed key before Explorer reacquires it.
		if enabled && keysAttempted {
			err = errors.Join(err, setHotkeys(cleanup, oldKeys))
		}
		_, rollbackErr := system.FinishWinV(cleanup, receipt.Token, true)
		err = errors.Join(err, rollbackErr)
		if restartAttempted && rollbackErr == nil {
			err = errors.Join(err, system.RestartExplorer(cleanup))
		}
		if !enabled && keysAttempted {
			err = errors.Join(err, setHotkeys(cleanup, oldKeys))
		}
		if preferenceAttempted {
			err = errors.Join(err, setPreference(cleanup, oldPreference))
		}
	}()
	// Disabling must release Win+V before Windows restores its own shortcut.
	if !enabled && newKeys != oldKeys {
		keysAttempted = true
		if err := setHotkeys(ctx, newKeys); err != nil {
			return err
		}
	}
	if receipt.Value {
		restartAttempted = true
		if err := system.RestartExplorer(ctx); err != nil {
			return err
		}
	}
	if enabled {
		keysAttempted = true
		if err := setHotkeys(ctx, newKeys); err != nil {
			return err
		}
	}
	preferenceAttempted = true
	if err := setPreference(ctx, strconv.FormatBool(enabled)); err != nil {
		return err
	}
	_, err = system.FinishWinV(ctx, receipt.Token, false)
	return err
}
