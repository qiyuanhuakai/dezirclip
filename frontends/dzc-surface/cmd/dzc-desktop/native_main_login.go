package main

import (
	"context"
	"encoding/json"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

type nativeLoginState struct {
	enabled, known, pending, failed bool
}

func (n *nativeMain) loginSettings(c *ui.Context) {
	state := &n.login
	if !state.known && !state.pending && !state.failed {
		n.loadLogin("")
	}
	enabled := state.enabled
	if n.mainKit().SettingsToggleField(c, &enabled, n.text("autostart"), "", "", !state.known || state.pending || n.mutating) {
		action := "disable-login"
		if enabled {
			action = "enable-login"
		}
		n.loadLogin(action)
	}
	if state.failed {
		ui.Row(c.Key("login-failure")).Gap(6).Wrap().Transition(nativeSettingReveal).Children(func() {
			ui.Text(c, n.text("native_autostart_failed")).TextColor(c.Theme().TextMuted).Wrap()
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_retry"), Disabled: state.pending}).Clicked() {
				n.loadLogin("")
			}
		})
	}
}

func (n *nativeMain) loadLogin(action string) {
	if n.session == nil || n.history.state.ServiceInstanceId == "" || n.login.pending {
		return
	}
	n.login.pending, n.login.failed = true, false
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 20*time.Second)
		defer cancel()
		var writeErr error
		if action != "" {
			_, writeErr = client.PackageTool(ctx, action)
		}
		raw, err := client.PackageTool(ctx, "login-status")
		var status *bool
		if err == nil {
			err = json.Unmarshal(raw, &status)
		}
		n.update(session, func() {
			n.login.pending, n.login.failed = false, writeErr != nil || err != nil || status == nil
			n.login.known = err == nil && status != nil
			if n.login.known {
				n.login.enabled = *status
			}
		})
	}()
}
