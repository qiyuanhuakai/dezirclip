package main

import (
	"context"
	"encoding/json"
	"log"
	"runtime"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

const nativePasteMethodKey = "app.paste_method"

func (n *nativeMain) pasteMethodSetting(c *ui.Context, kit uikit.Kit) {
	options := []uikit.SelectOption{
		{Value: "shift_insert", Label: n.text("paste_method_shift_insert")},
		{Value: "ctrl_v", Label: n.text("paste_method_ctrl_v")},
	}
	saved := n.preferences[nativePasteMethodKey]
	if runtime.GOOS == "windows" {
		options = append(options, uikit.SelectOption{Value: "game_mode", Label: n.text("paste_method_game_mode")})
	} else if saved == "game_mode" {
		// X11 uses the Shift+Insert path for this Windows setting.
		saved = "shift_insert"
	}
	if saved == "" {
		saved = "shift_insert"
	}
	selected, label := n.displayedSetting(nativePasteMethodKey, saved), n.text("paste_method")
	kit.SettingsField(c.Key(nativePasteMethodKey), uikit.SettingsFieldOptions{}).Children(func() {
		kit.SettingsInlineLabel(c, label, n.settingsHelpLabel(label), n.text("paste_method_hint"))
		if kit.Select(c, &selected, label, options, n.preferenceUnavailable(nativePasteMethodKey)).Changed() && selected != saved {
			if selected == "game_mode" {
				n.enableGameMode(saved)
			} else {
				n.saveSetting(nativePasteMethodKey, selected)
			}
		}
	})
}

func (n *nativeMain) enableGameMode(previous string) {
	state := n.history.state
	if n.preferenceUnavailable(nativePasteMethodKey) {
		return
	}
	n.mutating, n.notice = true, ""
	session, language := n.session, n.language()
	go func() {
		changed, err := n.saveGameMode(session, state, language, previous)
		n.update(session, func() {
			n.mutating = false
			if changed {
				n.refreshViewSettings()
			}
			if err != nil {
				n.notice = n.text("native_setting_save_failed")
			}
		})
	}()
}

func (n *nativeMain) saveGameMode(session context.Context, state protocol.RuntimeState, language nativeLanguage, previous string) (bool, error) {
	ctx, cancel := context.WithTimeout(session, 4*time.Second)
	admin, err := (&SystemSettings{owner: n.library.owner}).IsAdmin(ctx)
	cancel()
	if err != nil {
		return false, err
	}
	if !admin {
		n.shell.dock.dialogs.Add(1)
		result, err := mygo.Dialog.Message(mygo.MessageOptions{
			Parent: n.window, Title: language.text("admin_required"),
			Message: language.text("game_mode_admin_required"), Type: mygo.MessageWarning,
			Buttons:       []string{language.text("common_confirm"), language.text("common_cancel")},
			DefaultButton: 1, CancelButton: 1,
		})
		n.shell.dock.dialogs.Add(-1)
		if err != nil || result.Button != 0 {
			return false, err
		}
	}
	write, cancel := context.WithTimeout(session, 4*time.Second)
	err = n.library.SetSetting(write, nativePasteMethodKey, "game_mode", state.PrivacyEpoch)
	cancel()
	if err != nil {
		return false, err
	}
	if admin {
		return true, nil
	}
	// Once consent is given, a window hide must not interrupt the UAC exchange
	// or the corresponding rollback if the restart is declined.
	err = n.shell.RestartAsAdmin(context.Background())
	if err != nil {
		n.restorePasteMethod(state.ServiceInstanceId, previous)
	}
	return true, err
}

func (n *nativeMain) restorePasteMethod(instance, previous string) {
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
	defer cancel()
	status, err := n.library.Status(ctx)
	if err == nil && status.State.ServiceInstanceId == instance {
		var current map[string]string
		raw, readErr := n.library.preferences(ctx, status.State)
		err = readErr
		if err == nil {
			err = json.Unmarshal(raw, &current)
		}
		if err == nil && current[nativePasteMethodKey] == "game_mode" {
			err = n.library.SetSetting(ctx, nativePasteMethodKey, previous, status.State.PrivacyEpoch)
		}
	}
	if err != nil {
		log.Printf("Could not restore the previous paste method: %v", err)
	}
}

type nativeQueueCount struct {
	events                 uint64
	count                  uint32
	known, reading, failed bool
}

func (q *nativeQueueCount) observe(status string) bool {
	if count, ok := strings.CutPrefix(status, "paste_queue_changed:"); ok {
		value, err := strconv.ParseUint(count, 10, 32)
		if err == nil && value <= 32768 {
			changed := !q.known || q.count != uint32(value)
			q.events++
			q.count, q.known, q.failed = uint32(value), true, false
			return changed
		}
	}
	return false
}

func (q *nativeQueueCount) finishRead(token uint64, count uint32, err error) {
	q.reading = false
	if token == q.events {
		q.count, q.known, q.failed = count, err == nil, err != nil
	}
}

func (n *nativeMain) loadQueueCount() {
	state := n.history.state
	if n.session == nil || state.ServiceInstanceId == "" || n.queueCount.reading {
		return
	}
	n.queueCount.reading, n.queueCount.failed = true, false
	session, token := n.session, n.queueCount.events
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		status, err := n.library.queueStatus(ctx, state)
		n.update(session, func() { n.queueCount.finishRead(token, status.Count, err) })
	}()
}

func (n *nativeMain) sequentialSettings(c *ui.Context) {
	const setting = "app.sequential_mode"
	n.booleanSettingWithHelp(c, setting, n.text("sequential_paste_mode"), n.text("sequential_paste_mode_hint"), false)
	if !n.settingEnabled(setting, false) {
		return
	}
	if !n.queueCount.known && !n.queueCount.failed {
		n.loadQueueCount()
	}
	switch {
	case n.queueCount.failed:
		ui.Column(c.Key("sequential-queue-error")).ColumnSpan(-1).Gap(6).Transition(nativeSettingReveal).Children(func() {
			ui.Text(c, n.text("sequential_paste_queue_unavailable")).TextColor(c.Theme().Danger).Wrap()
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_retry"), Disabled: n.mutating}).Clicked() {
				n.loadQueueCount()
			}
		})
	case n.queueCount.known && n.queueCount.count > 0:
		kit := n.mainKit()
		kit.SettingsField(c.Key("sequential-queue"), uikit.SettingsFieldOptions{}).
			ColumnSpan(-1).Transition(nativeSettingReveal).Children(func() {
			label := strings.ReplaceAll(n.text("sequential_paste_queue_count"), "{count}", strconv.FormatUint(uint64(n.queueCount.count), 10))
			kit.SettingsLabel(c, label).Grow(1).Wrap()
			if kit.Button(c, uikit.ButtonOptions{
				Label: n.text("sequential_paste_clear_queue"), Disabled: n.mutating,
			}).Clicked() {
				n.clearNativePasteQueue()
			}
		})
	}
	n.shortcutSetting(c, "app.sequential_hotkey", n.text("sequential_paste_hotkey_label"), "Alt+V")
}

func (n *nativeMain) clearNativePasteQueue() {
	state := n.history.state
	if n.session == nil || state.ServiceInstanceId == "" || n.mutating {
		return
	}
	n.mutating, n.notice = true, ""
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		err := n.library.SetPasteQueue(ctx, state.PrivacyEpoch, []int64{})
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = n.text("sequential_paste_queue_unavailable")
			}
		})
	}()
}

func (n *nativeMain) retentionSettings(c *ui.Context) {
	n.booleanSettingWithHelp(c, "app.persistent", n.text("persistent_storage", "Keep clipboard history"), n.text("persistent_storage_hint"), false)
	if !n.settingEnabled("app.persistent", false) {
		return
	}
	ui.Column(c.Key("persistent-limit-toggle")).MinWidth(0).Transition(nativeSettingReveal).Children(func() {
		n.booleanSettingWithHelp(c, "app.persistent_limit_enabled", n.text("persistent_limit_enabled", "Limit stored clipboard entries"), n.text("persistent_limit_enabled_hint"), true)
	})
	if !n.settingEnabled("app.persistent_limit_enabled", true) {
		return
	}
	n.numberSetting(c, "app.persistent_limit", n.text("persistent_limit"), n.text("persistent_limit_hint"), 500, 50, 99999)
}

func (n *nativeMain) numberSetting(c *ui.Context, key, label, hint string, fallback, minimum, maximum int) {
	saved := n.preferences[key]
	if saved == "" {
		saved = strconv.Itoa(fallback)
	}
	type limitDraft struct {
		value, saved string
	}
	kit := n.mainKit()
	row := kit.SettingsField(c.Key(key), uikit.SettingsFieldOptions{Detail: key == "app.idle_destroy_seconds"}).Transition(nativeSettingReveal)
	draft := ui.Local(row, key, func() limitDraft { return limitDraft{value: saved, saved: saved} })
	if draft.saved != saved {
		draft.value, draft.saved = saved, saved
	}
	row.Children(func() {
		kit.SettingsInlineLabel(c, label, n.settingsHelpLabel(label), hint)
		input, committed := kit.SettingsNumberField(c, &draft.value, label, n.mutating)
		input.Width(112)
		if !committed {
			return
		}
		limit, valid := nativeBoundedNumber(draft.value, minimum, maximum)
		if !valid || limit == saved || n.preferenceUnavailable(key) {
			draft.value = saved
			return
		}
		draft.value = limit
		n.savePreference(key, limit, 4*time.Second, func(ctx context.Context, epoch uint64) error {
			return n.library.SetSetting(ctx, key, limit, epoch)
		}, func(err error) {
			if err != nil && draft.value == limit {
				draft.value = saved
			}
		})
	})
}

func nativeBoundedNumber(value string, minimum, maximum int) (string, bool) {
	raw := strings.TrimSpace(value)
	if raw == "" {
		return "", false
	}
	for _, digit := range raw {
		if digit < '0' || digit > '9' {
			return "", false
		}
	}
	number, err := strconv.ParseUint(raw, 10, 64)
	if err != nil || number > uint64(maximum) {
		return strconv.Itoa(maximum), true
	}
	return strconv.Itoa(max(minimum, int(number))), true
}
