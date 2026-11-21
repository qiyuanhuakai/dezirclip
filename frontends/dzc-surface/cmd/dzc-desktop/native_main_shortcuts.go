package main

import (
	"context"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

const mainShortcutSetting = "app.hotkey"

type nativeShortcutForm struct {
	key, label string
	open       bool
	listening  bool
	focus      bool
	manual     bool
	typed      string
	cancel     context.CancelFunc
	captured   chan string
}

func (n *nativeMain) shortcutAvailable() bool {
	return n.session != nil && n.history.state.ServiceInstanceId != "" && !n.mutating &&
		n.shortcut == nil && n.privacyDialog == nil && n.backup == nil && !n.winV.changing
}

func (n *nativeMain) shortcutValue(key, fallback string) string {
	if value, stored := n.preferences[key]; stored {
		return value
	}
	return fallback
}

func (n *nativeMain) globalShortcutSettings(c *ui.Context) {
	label := n.text("global_hotkey")
	kit := n.mainKit()
	bindings := nativeMainChords(n.shortcutValue(mainShortcutSetting, ""))
	disabled := !n.shortcutAvailable()
	ui.Column(c).ColumnSpan(-1).Children(func() {
		kit.HotkeyListEditor(c, label, func() {
			if kit.HotkeyBinding(c, "+ "+label, "", "+ "+label, false, disabled).Clicked() {
				n.openShortcut(mainShortcutSetting, label)
			}
		}, func() {
			kit.HotkeyListWithOptions(c, uikit.HotkeyListOptions{
				Values: bindings, Placeholder: n.text("not_set"), RemoveLabel: n.text("delete"),
				Disabled: disabled, CanRemove: n.canRemoveMainShortcut,
				OnRemove: func(chord string) {
					updated := slices.DeleteFunc(slices.Clone(bindings), func(value string) bool { return value == chord })
					n.saveNativeShortcut(mainShortcutSetting, strings.Join(updated, "\n"))
				},
			})
		})
	})
	n.winVSettings(c, kit)
}

func nativeMainChords(raw string) []string {
	parts := strings.FieldsFunc(raw, func(r rune) bool { return r == '\r' || r == '\n' })
	for i, part := range parts {
		parts[i] = mainShortcutName(part)
	}
	return slices.DeleteFunc(parts, func(chord string) bool { return chord == "" })
}

func (n *nativeMain) shortcutSetting(c *ui.Context, key, label, fallback string) {
	n.shortcutSettingWithOptions(c, key, label, fallback, uikit.HotkeyRowOptions{})
}

func (n *nativeMain) shortcutSettingWithOptions(c *ui.Context, key, label, fallback string, options uikit.HotkeyRowOptions) {
	value := n.shortcutValue(key, fallback)
	options.Disabled = !n.shortcutAvailable()
	if value == "" {
		options.OnClear = nil
	}
	kit := n.mainKit()
	kit.HotkeyRowWithOptions(c, label, options, func() {
		control := kit.HotkeyBinding(c, label, value, n.text("not_set"), false, options.Disabled).
			Tooltip(n.text("hotkey_click_hint"))
		if control.Clicked() {
			n.openShortcut(key, label)
		}
	})
}

func (n *nativeMain) openShortcut(key, label string) {
	if !n.shortcutAvailable() || n.window == nil || !mainWindowFocused(n.window) {
		return
	}
	n.notice = ""
	session, state := n.session, n.history.state
	lifetime, cancel := context.WithTimeout(session, 45*time.Second)
	form := &nativeShortcutForm{
		key: key, label: label, open: true, cancel: cancel, captured: make(chan string, 1),
	}
	n.shortcut = form
	go n.recordNativeShortcut(session, lifetime, state, form)
}

func (n *nativeMain) recordNativeShortcut(session, lifetime context.Context, state protocol.RuntimeState, form *nativeShortcutForm) {
	if lifetime.Err() != nil {
		n.finishNativeShortcut(session, lifetime, form, "", nil)
		return
	}
	// Complete both resident requests in order even if the window hides while
	// the first acknowledgement is in flight.
	err := n.shell.changeRecordingMode(context.Background(), true, state.PrivacyEpoch)
	var chord string
	if err == nil {
		n.update(session, func() {
			if n.shortcut == form && form.open && lifetime.Err() == nil {
				form.listening, form.focus = true, true
				n.installShortcutInput()
			}
		})
		chord = n.waitForNativeShortcut(lifetime, form.captured)
	}
	stopErr := n.shell.changeRecordingMode(context.Background(), false, state.PrivacyEpoch)
	if err == nil {
		err = stopErr
	}
	n.finishNativeShortcut(session, lifetime, form, chord, err)
}

func (n *nativeMain) waitForNativeShortcut(ctx context.Context, captured <-chan string) string {
	// X11 focus events can precede the window manager's update of
	// _NET_ACTIVE_WINDOW. Recheck only during recording.
	focus := time.NewTicker(120 * time.Millisecond)
	defer focus.Stop()
	for {
		select {
		case chord := <-captured:
			return chord
		case <-ctx.Done():
			return ""
		case <-focus.C:
			if !mainWindowFocused(n.window) {
				return ""
			}
		}
	}
}

func (n *nativeMain) finishNativeShortcut(session, lifetime context.Context, form *nativeShortcutForm, chord string, err error) {
	n.window.Update(func() {
		if n.window.IsDestroyed() || n.shortcut != form {
			return
		}
		cancelled := lifetime.Err() != nil || !form.open || n.session != session
		form.cancel()
		n.shortcut = nil
		if cancelled || !mainWindowFocused(n.window) {
			return
		}
		if err != nil {
			n.notice = n.shortcutError(err)
			return
		}
		if chord != "" {
			n.applyRecordedShortcut(form.key, chord)
		}
	})
}

func (n *nativeMain) cancelShortcut() {
	if form := n.shortcut; form != nil {
		form.open, form.listening = false, false
		form.cancel()
	}
}

func (n *nativeMain) receiveShortcut(event recordingEvent) {
	if form := n.shortcut; form != nil && form.open {
		if event.Cancelled {
			n.cancelShortcut()
			n.window.Invalidate()
		} else if !form.manual {
			n.captureNativeShortcut(form, event.Hotkey)
		}
	}
}

func (n *nativeMain) captureNativeShortcut(form *nativeShortcutForm, chord string) {
	if n.shortcut == form && form.open && form.listening && chord != "" {
		if !mainWindowFocused(n.window) {
			n.cancelShortcut()
			return
		}
		form.listening = false
		form.captured <- chord
	}
}

func (n *nativeMain) drawShortcutRecorder(c *ui.Context, kit uikit.Kit, form *nativeShortcutForm) {
	kit.Dialog(c, &form.open, form.label, "", false, func() {
		if form.manual {
			n.drawTypedShortcut(c, form)
		} else {
			n.drawRecordedShortcut(c, form)
		}
		ui.Row(c).Gap(6).Wrap().Children(func() {
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_cancel")}).Clicked() {
				form.open = false
			}
			mode := n.text("native_shortcut_type")
			if form.manual {
				mode = n.text("native_shortcut_record")
			}
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: mode, Disabled: !form.listening}).Clicked() {
				form.manual = !form.manual
				form.focus = true
			}
		})
	})
	if !form.open {
		n.cancelShortcut()
	}
}

func (n *nativeMain) drawRecordedShortcut(c *ui.Context, form *nativeShortcutForm) {
	status := n.text("common_loading")
	if form.listening {
		status = n.text("waiting_for_input")
	}
	field := n.mainKit().HotkeyBinding(c, status, "", status, true, false).FillWidth()
	field.HandleInput(func(event ui.InputEvent) bool {
		switch {
		case event.Kind == ui.InputKeyDown && event.Key == ui.KeyEscape && event.Mods == 0:
			n.cancelShortcut()
			return true
		case event.Kind == ui.InputKeyDown:
			if !event.Repeat {
				n.captureNativeShortcut(form, nativeShortcutChord(event.Key, event.Mods))
			}
			return true
		case event.Kind == ui.InputPointerDown && event.Button == 2 && event.Mods == 0 && form.key == mainShortcutSetting:
			n.captureNativeShortcut(form, "MouseMiddle")
			return true
		default:
			return false
		}
	})
	if form.focus {
		field.Focus()
		form.focus = false
	}
	ui.Text(c, n.text("hotkey_recording_esc")).TextColor(c.Theme().TextMuted).Wrap()
}

func (n *nativeMain) drawTypedShortcut(c *ui.Context, form *nativeShortcutForm) {
	field := n.mainKit().TextField(c, &form.typed, n.text("native_shortcut_input")).FillWidth().Placeholder("Ctrl+Alt+K")
	if form.focus {
		field.Focus()
		form.focus = false
	}
	chord := typedShortcutChord(form.typed)
	if n.mainKit().Button(c, uikit.ButtonOptions{
		Label: n.text("common_save"), Primary: true, Disabled: !form.listening || chord == "",
	}).Clicked() || field.Submitted() {
		n.captureNativeShortcut(form, chord)
	}
}

func typedShortcutChord(value string) string {
	if len(value) > 128 || strings.ContainsAny(value, "\r\n\x00") {
		return ""
	}
	return mainShortcutName(value)
}

func nativeShortcutChord(key ui.Key, mods ui.Modifiers) string {
	var name string
	switch {
	case key >= ui.KeyA && key <= ui.KeyZ:
		name = string(rune('A' + key - ui.KeyA))
	case key >= ui.Key0 && key <= ui.Key9:
		name = string(rune('0' + key - ui.Key0))
	case key >= ui.KeyF1 && key <= ui.KeyF12:
		name = "F" + strconv.Itoa(int(key-ui.KeyF1)+1)
	default:
		name = map[ui.Key]string{
			ui.KeyEnter: "Enter", ui.KeyBackspace: "Backspace", ui.KeyTab: "Tab", ui.KeySpace: "Space",
			ui.KeyDelete: "Delete", ui.KeyInsert: "Insert", ui.KeyHome: "Home", ui.KeyEnd: "End",
			ui.KeyPageUp: "PageUp", ui.KeyPageDown: "PageDown", ui.KeyLeft: "Left", ui.KeyRight: "Right",
			ui.KeyUp: "Up", ui.KeyDown: "Down", ui.KeyEscape: "Escape",
			ui.KeyBackquote: "Backquote", ui.KeyMinus: "Minus", ui.KeyEqual: "Equal",
			ui.KeyBracketLeft: "BracketLeft", ui.KeyBracketRight: "BracketRight", ui.KeyBackslash: "Backslash",
			ui.KeySemicolon: "Semicolon", ui.KeyQuote: "Quote", ui.KeyComma: "Comma", ui.KeyPeriod: "Period",
			ui.KeySlash: "Slash", ui.KeyContextMenu: "ContextMenu",
		}[key]
	}
	if name == "" || (name == "Escape" && mods == 0) {
		return ""
	}
	return shortcutChord(name, mods)
}

func shortcutChord(name string, mods ui.Modifiers) string {
	var keys []string
	for _, modifier := range []struct {
		code ui.Modifiers
		name string
	}{{ui.Ctrl, "Ctrl"}, {ui.Alt, "Alt"}, {ui.Shift, "Shift"}, {ui.Super, "Super"}} {
		if mods&modifier.code != 0 {
			keys = append(keys, modifier.name)
		}
	}
	return strings.Join(append(keys, name), "+")
}

func (n *nativeMain) applyRecordedShortcut(key, recorded string) {
	chord := mainShortcutName(recorded)
	if key != mainShortcutSetting {
		if chord == "MouseMiddle" {
			n.notice = n.text("hotkey_unavailable")
			return
		}
		if chord != mainShortcutName(n.shortcutValue(key, "")) {
			n.saveNativeShortcut(key, chord)
		}
		return
	}
	chords := nativeMainChords(n.shortcutValue(key, ""))
	if slices.Contains(chords, chord) {
		n.notice = n.text("privacy_hotkey_busy")
		return
	}
	n.saveNativeShortcut(key, strings.Join(append(chords, chord), "\n"))
}

func (n *nativeMain) shortcutError(err error) string {
	reason := strings.ToLower(err.Error())
	if strings.Contains(reason, "busy") || strings.Contains(reason, "already") ||
		strings.Contains(reason, "conflict") || strings.Contains(reason, "in use") {
		return n.text("privacy_hotkey_busy")
	}
	return n.text("hotkey_unavailable")
}

func (n *nativeMain) saveNativeShortcut(key, value string) {
	if !n.shortcutAvailable() {
		return
	}
	session, state := n.session, n.history.state
	n.mutating, n.notice = true, ""
	go func() {
		ctx, cancel := context.WithTimeout(session, 15*time.Second)
		defer cancel()
		var err error
		switch key {
		case mainShortcutSetting:
			err = n.shell.SetMainHotkeys(ctx, value, state.PrivacyEpoch)
		case "app.search_hotkey":
			err = n.shell.SetSearchHotkey(ctx, value, state.PrivacyEpoch)
		case "app.sequential_hotkey", "app.rich_paste_hotkey":
			err = n.shell.SetPasteHotkey(ctx, key, value, state.PrivacyEpoch)
		case "app.privacy_session_hotkey":
			_, err = n.library.SetPrivacyHotkey(ctx, value)
		default:
			err = n.shell.SetPanelHotkey(ctx, key, value, state.PrivacyEpoch)
		}
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = n.shortcutError(err)
				return
			}
			n.prepare(n.resume)
		})
	}()
}
