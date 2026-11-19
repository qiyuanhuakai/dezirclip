package main

import (
	"context"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeQueueCount struct {
	events                 uint64
	count                  uint32
	known, reading, failed bool
}

func (q *nativeQueueCount) observe(status string) {
	if count, ok := strings.CutPrefix(status, "paste_queue_changed:"); ok {
		value, err := strconv.ParseUint(count, 10, 32)
		if err == nil && value <= 32768 {
			q.events++
			q.count, q.known, q.failed = uint32(value), true, false
		}
	}
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
	n.booleanSettings(c, nativeBooleanSetting{"app.sequential_mode", n.text("sequential_paste_mode"), false})
	if !n.settingEnabled("app.sequential_mode", false) {
		return
	}
	if !n.queueCount.known && !n.queueCount.failed {
		n.loadQueueCount()
	}
	switch {
	case n.queueCount.failed:
		ui.Text(c, n.text("sequential_paste_queue_unavailable")).TextColor(c.Theme().Danger).Wrap()
		if uikit.Button(c, uikit.ButtonOptions{Label: n.text("common_retry"), Disabled: n.mutating}).Clicked() {
			n.loadQueueCount()
		}
	case n.queueCount.known:
		ui.Row(c).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
			ui.Text(c, strings.ReplaceAll(n.text("sequential_paste_queue_count"), "{count}", strconv.FormatUint(uint64(n.queueCount.count), 10)))
			if uikit.Button(c, uikit.ButtonOptions{
				Label: n.text("sequential_paste_clear_queue"), Disabled: n.queueCount.count == 0 || n.mutating,
			}).Clicked() {
				n.clearNativePasteQueue()
			}
		})
	}
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
	n.booleanSettings(c, nativeBooleanSetting{"app.persistent", n.text("persistent_storage", "Keep clipboard history"), false})
	if !n.settingEnabled("app.persistent", false) {
		return
	}
	n.booleanSettings(c, nativeBooleanSetting{"app.persistent_limit_enabled", n.text("persistent_limit_enabled", "Limit stored clipboard entries"), true})
	if !n.settingEnabled("app.persistent_limit_enabled", true) {
		return
	}
	const key = "app.persistent_limit"
	saved := n.preferences[key]
	if saved == "" {
		saved = "500"
	}
	label := n.text("persistent_limit")
	ui.Text(c, label)
	type limitDraft struct {
		value, saved string
	}
	row := ui.Row(c).FillWidth().Gap(6).Wrap()
	draft := ui.Local(row, key, func() limitDraft { return limitDraft{value: saved, saved: saved} })
	if draft.saved != saved {
		draft.value, draft.saved = saved, saved
	}
	row.Children(func() {
		input := ui.TextInput(c, &draft.value).Label(label).Width(112).ReadOnly(n.mutating)
		limit, valid := nativeStorageLimit(draft.value)
		save := uikit.Button(c, uikit.ButtonOptions{Label: n.text("common_save"), Disabled: n.mutating || !valid || draft.value == saved})
		if save.Clicked() || input.Submitted() && valid && !n.mutating {
			draft.value = limit
			if limit != saved {
				n.saveSetting(key, limit)
			}
		}
	})
}

func nativeStorageLimit(value string) (string, bool) {
	raw := strings.TrimSpace(value)
	if raw == "" {
		return "", false
	}
	for _, digit := range raw {
		if digit < '0' || digit > '9' {
			return "", false
		}
	}
	raw = strings.TrimLeft(raw, "0")
	if len(raw) > 5 {
		return "99999", true
	}
	n, _ := strconv.Atoi(raw)
	return strconv.Itoa(max(50, n)), true
}
