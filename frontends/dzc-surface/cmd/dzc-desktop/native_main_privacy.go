package main

import (
	"context"
	"errors"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativePrivacyRules struct {
	started, loading, failed bool
	rules                    []protocol.PolicyRuleValue
	pendingDelete            string
	actionFailed             bool
}

type nativePrivacyDialog struct {
	open, loading, failed bool
	cancel                context.CancelFunc
	params                protocol.DesktopPolicyParams
	source, hint          string
	prompt                string
}

type nativePrivacyFeedbackAction struct {
	id, label, prompt string
}

const (
	sourceExclusionsSetting = "app.excluded_source_apps"
	customPrivacySetting    = "app.privacy_protection_custom_rules"
	privacyTextDelay        = 250 * time.Millisecond
	privacyTextTimeout      = 4 * time.Second
)

type nativePrivacyTextDraft struct {
	instance, value                   string
	editing, composing, dirty, failed bool
	private                           bool
	ready                             time.Time
	pending                           *nativePrivacyTextWrite
}

type nativePrivacyTextWrite struct {
	value string
	done  chan struct{}
	err   error
}

func (n *nativeMain) privacyText(draft *nativePrivacyTextDraft, key string) string {
	saved := n.preferences[key]
	instance := n.history.state.ServiceInstanceId
	if instance != "" && instance != draft.instance {
		*draft = nativePrivacyTextDraft{instance: instance, value: saved, private: n.private}
	}
	if !draft.dirty && draft.pending == nil {
		draft.value = saved
	}
	return saved
}

func (n *nativeMain) observePrivacyText(draft *nativePrivacyTextDraft, field ui.Element, previous, saved string, delay time.Duration) {
	draft.composing = field.Composing()
	draft.editing = field.Focused() || draft.composing
	if draft.value == previous {
		return
	}
	draft.dirty, draft.failed = draft.value != saved, false
	if n.history.state.ServiceInstanceId == draft.instance {
		draft.private = n.private
	}
	if delay == 0 {
		return
	}
	draft.ready = time.Now().Add(delay)
	if n.window != nil {
		if n.privacyTextTimer != nil {
			n.privacyTextTimer.Stop()
		}
		n.privacyTextTimer = time.AfterFunc(delay, func() {
			n.window.Update(func() {
				if !n.window.IsDestroyed() {
					n.savePrivacyTexts(false)
				}
			})
		})
	}
}

func (n *nativeMain) drawSourceExclusions(c *ui.Context, kit uikit.Kit) {
	label := n.text("excluded_source_apps")
	hint := n.text("excluded_source_apps_hint") + "\n" + n.text("privacy_rules_legacy_exclusions_hint")
	draft := &n.sourceExclusions
	saved := n.privacyText(draft, sourceExclusionsSetting)
	disabled := n.session == nil || draft.instance == "" || n.mutating
	kit.SettingsField(c.Key(sourceExclusionsSetting), uikit.SettingsFieldOptions{Stacked: true}).
		ColumnSpan(-1).Children(func() {
		kit.SettingsFieldLabel(c, label, n.settingsHelpLabel(label), hint)
		previous := draft.value
		field := kit.SourceExclusionField(c, &draft.value, label, n.text("excluded_source_apps_placeholder")).ReadOnly(disabled)
		n.observePrivacyText(draft, field, previous, saved, 0)
		n.drawPrivacyTextRetry(c, kit, draft, disabled)
	})
}

func (n *nativeMain) drawPrivacyTextRetry(c *ui.Context, kit uikit.Kit, draft *nativePrivacyTextDraft, disabled bool) {
	if !draft.failed {
		return
	}
	ui.Text(c, n.text("privacy_rules_save_failed")).Wrap().TextColor(c.Theme().Danger)
	if kit.Button(c, uikit.ButtonOptions{Label: n.text("common_retry"), Disabled: disabled || n.history.state.ServiceInstanceId != draft.instance}).Clicked() {
		draft.failed, draft.private = false, n.private
	}
}

func (n *nativeMain) savePrivacyTexts(closing bool) {
	instance := n.history.state.ServiceInstanceId
	if n.library == nil || n.window == nil || n.profile.moving || n.profile.restartPending ||
		!closing && (n.session == nil || instance == "" || n.mutating) {
		return
	}
	n.savePrivacyText(&n.sourceExclusions, sourceExclusionsSetting, instance, closing)
	n.savePrivacyText(&n.customPrivacyRules, customPrivacySetting, instance, closing)
}

func (n *nativeMain) savePrivacyText(draft *nativePrivacyTextDraft, key, instance string, closing bool) {
	pending := draft.pending
	if draft.instance == "" || instance != "" && draft.instance != instance || draft.failed ||
		pending == nil && !draft.dirty || pending != nil && (!closing || pending.value == draft.value) {
		return
	}
	if !closing && (draft.composing || draft.editing && (draft.ready.IsZero() || time.Now().Before(draft.ready))) {
		return
	}
	write := &nativePrivacyTextWrite{value: draft.value, done: make(chan struct{})}
	previous := n.privacyTextWrite
	draft.pending, n.privacyTextWrite = write, write
	instance, private := draft.instance, draft.private
	window, library := n.window, n.library
	ctx, cancel := context.WithTimeout(context.Background(), privacyTextTimeout)
	go func() {
		// The capture setting belongs to the authenticated profile even when its window hides.
		defer cancel()
		if previous != nil {
			select {
			case <-previous.done:
			case <-ctx.Done():
			}
		}
		// Capture policy edits advance the epoch. Read it after the prior write
		// and keep the original Service and privacy mode of the edit.
		status, err := library.Status(ctx)
		if err == nil && (status.State.ServiceInstanceId != instance || status.Privacy.Active != private) {
			err = errors.New("PrivacyEpochChanged")
		}
		write.err = err
		if err == nil {
			write.err = library.edit(ctx, protocol.MethodDesktopSettingsCaptureSet, protocol.CaptureSettingParams{
				ServiceInstanceId: instance, ExpectedEpoch: status.State.PrivacyEpoch, Key: key, Value: write.value,
			})
		}
		close(write.done)
		window.Update(func() {
			if window.IsDestroyed() || draft.pending != write {
				return
			}
			draft.pending = nil
			if write.err == nil {
				draft.dirty = draft.value != write.value
				if current := n.history.state.ServiceInstanceId; current == "" || current == instance {
					n.preferences[key] = write.value
				}
			} else if draft.value == write.value {
				draft.failed = true
			}
		})
	}()
}

func (n *nativeMain) waitForPrivacyTexts() {
	if n.privacyTextTimer != nil {
		n.privacyTextTimer.Stop()
	}
	if pending := n.privacyTextWrite; pending != nil {
		timer := time.NewTimer(privacyTextTimeout)
		defer timer.Stop()
		select {
		case <-pending.done:
		case <-timer.C:
		}
	}
}

func (n *nativeMain) privacyFeedbackActions() []nativePrivacyFeedbackAction {
	return []nativePrivacyFeedbackAction{
		{"not_sensitive", n.text("privacy_feedback_not_sensitive"), n.text("privacy_feedback_unprotect_confirm")},
		{"exact_exception", n.text("privacy_feedback_exact_exception"), n.text("privacy_feedback_exact_confirm")},
		{"ignore_source", n.text("privacy_feedback_ignore_source"), n.text("privacy_feedback_ignore_confirm")},
		{"protect_source", n.text("privacy_feedback_protect_source"), n.text("privacy_feedback_protect_confirm")},
	}
}

func (n *nativeMain) openPrivacyFeedback(row protocol.DesktopEntry, generation uint64, action nativePrivacyFeedbackAction) {
	if !n.canOpenHistoryDialog() || generation != n.history.generation || row.Id == 0 {
		return
	}
	n.resetPreviewSelection()
	n.notice = ""
	session, state := n.session, n.history.state
	ctx, cancel := context.WithTimeout(session, 4*time.Second)
	id := row.Id
	dialog := &nativePrivacyDialog{
		open: true, loading: true, cancel: cancel, source: n.privacySource(row.SourceApp),
		params: protocol.DesktopPolicyParams{
			Id: &id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Action: &action.id,
		},
	}
	dialog.prompt = strings.ReplaceAll(action.prompt, "{source}", dialog.source)
	n.privacyDialog = dialog
	params := dialog.params
	params.Action = nil
	go func() {
		defer cancel()
		decision, err := n.library.PolicyDecision(ctx, params)
		n.update(session, func() {
			if n.privacyDialog != dialog {
				return
			}
			dialog.loading, dialog.failed = false, err != nil
			switch decision.Reason {
			case "scan_incomplete":
				dialog.hint = n.text("privacy_scan_incomplete")
			case "suspected_sensitive":
				dialog.hint = n.text("privacy_scan_suspected")
			}
		})
	}()
}

func (n *nativeMain) privacySource(source string) string {
	if source = strings.TrimSpace(source); source != "" {
		return source
	}
	return n.text("privacy_source_unknown")
}

func (n *nativeMain) showPrivacyRuleManager() {
	n.closePrivacyDialog()
	n.openSettings()
	n.settingsSection = "privacy"
	for id, open := range n.settingsOpen {
		*open = id == "privacy"
	}
}

func (n *nativeMain) privacyDialogView(c *ui.Context, kit uikit.Kit, dialog *nativePrivacyDialog) {
	kit.Dialog(c, &dialog.open, n.text("privacy_feedback_title"), "", n.mutating, func() {
		_, height := c.Size()
		ui.Scroll(c).FillWidth().MaxHeight(max(72, height-146)).Children(func() {
			ui.Column(c).FillWidth().Gap(6).Children(func() {
				switch {
				case dialog.loading:
					ui.Text(c, n.text("common_loading")).Wrap()
				case dialog.failed:
					ui.Text(c, n.text("privacy_action_failed")).TextColor(c.Theme().Danger).Wrap()
				case dialog.prompt != "":
					if dialog.hint != "" {
						ui.Text(c, dialog.hint).TextColor(c.Theme().TextMuted).Wrap()
					}
					ui.Text(c, dialog.prompt).Wrap()
				}
			})
		})
		ui.Row(c).Gap(6).Wrap().Justify(ui.End).Children(func() {
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_cancel")}).Clicked() {
				dialog.open = false
			}
			if dialog.prompt != "" && !dialog.loading && !dialog.failed && n.mainKit().Button(c, uikit.ButtonOptions{
				Label: n.text("common_confirm"), Primary: true, Disabled: n.mutating,
			}).Clicked() {
				n.commitPrivacyDialog(dialog)
			}
		})
	})
	if !dialog.open && n.privacyDialog == dialog {
		n.closePrivacyDialog()
	}
}

func (n *nativeMain) closePrivacyDialog() {
	if dialog := n.privacyDialog; dialog != nil {
		if dialog.cancel != nil {
			dialog.cancel()
		}
		n.privacyDialog = nil
	}
}

func (n *nativeMain) commitPrivacyDialog(dialog *nativePrivacyDialog) {
	if n.session == nil || n.privacyDialog != dialog || n.mutating || dialog.loading || dialog.failed ||
		dialog.params.Id == nil || dialog.params.Action == nil {
		return
	}
	state, params := n.history.state, dialog.params
	if params.ServiceInstanceId != state.ServiceInstanceId || params.ExpectedEpoch != state.PrivacyEpoch {
		n.closePrivacyDialog()
		return
	}
	n.closePrivacyDialog()
	n.resetPreviewSelection()
	n.mutating, n.notice = true, ""
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 8*time.Second)
		defer cancel()
		_, err := n.library.PolicyFeedback(ctx, params)
		n.update(session, func() {
			n.prepare(n.resume)
			if err != nil {
				n.notice = n.text("privacy_rules_action_failed")
				return
			}
			n.notice = n.text("privacy_feedback_done")
		})
	}()
}

func (n *nativeMain) loadPrivacyRules() {
	state := n.history.state
	if n.session == nil || state.ServiceInstanceId == "" || n.privacyRules.loading {
		return
	}
	n.privacyRules = nativePrivacyRules{started: true, loading: true}
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		rules, err := n.library.PolicyRules(ctx, state.PrivacyEpoch)
		n.update(session, func() {
			n.privacyRules = nativePrivacyRules{started: true, failed: err != nil, rules: rules}
		})
	}()
}

func (n *nativeMain) drawPrivacyRules(c *ui.Context) {
	kit := n.mainKit()
	rules := &n.privacyRules
	if !rules.started {
		n.loadPrivacyRules()
	}
	var count *int
	if rules.started && !rules.loading && !rules.failed {
		total := len(rules.rules)
		count = &total
	}
	kit.PrivacyRulesHeading(c, n.text("privacy_rules_title"), count)
	switch {
	case !rules.started || rules.loading:
		ui.Text(c, n.text("privacy_rules_loading")).TextColor(c.Theme().TextMuted).Wrap()
	case rules.failed:
		ui.Text(c, n.text("privacy_rules_load_failed")).TextColor(c.Theme().Danger).Wrap()
		if kit.SettingsDataAction(c, n.text("privacy_retry"), "", n.mutating).Clicked() {
			n.loadPrivacyRules()
		}
	case len(rules.rules) == 0:
		ui.Text(c, n.text("privacy_rules_empty")).TextColor(c.Theme().TextMuted).Wrap()
	default:
		width, _ := c.Size()
		columns := 1
		if width >= 1000 {
			columns = 3
		} else if width >= 700 {
			columns = 2
		}
		kit.PrivacyRuleGrid(c, columns, func() {
			for _, rule := range rules.rules {
				n.drawPrivacyRule(c, kit, rule)
			}
		})
	}
	if rules.actionFailed {
		ui.Text(c, n.text("privacy_rules_action_failed")).TextColor(c.Theme().Danger).Wrap()
	}
}

func (n *nativeMain) drawPrivacyRule(c *ui.Context, kit uikit.Kit, rule protocol.PolicyRuleValue) {
	var name, scope string
	switch rule.Kind {
	case "exact_exception":
		name, scope = n.text("privacy_rule_exact_exception"), n.text("privacy_rule_scope_exact_exception")
	case "ignore_source":
		name, scope = n.text("privacy_rule_ignore_source"), n.text("privacy_rule_scope_ignore_source")
	case "protect_source":
		name, scope = n.text("privacy_rule_protect_source"), n.text("privacy_rule_scope_protect_source")
	default:
		return
	}
	source := n.text("privacy_source_unknown")
	if rule.SourceApp != nil {
		source = n.privacySource(*rule.SourceApp)
	}
	remove, prompt := n.text("privacy_rule_remove"), n.text("privacy_rule_delete_source_confirm")
	if rule.Kind == "exact_exception" {
		remove, prompt = n.text("privacy_rule_revoke_exception"), n.text("privacy_rule_delete_exact_confirm")
	}
	stamp := time.UnixMilli(rule.CreatedAtMs).Local().Format("2006-01-02 15:04")
	kit.PrivacyRuleCard(c, uikit.PrivacyRuleCardOptions{
		Title: name, Source: source, Scope: scope,
		Identifier:  n.text("privacy_rule_identifier") + " " + rule.Id[:min(8, len(rule.Id))],
		Created:     n.text("privacy_rule_created") + " " + stamp,
		RemoveLabel: remove, CancelLabel: n.text("cancel"), ConfirmLabel: n.text("privacy_rule_delete_title"),
		Prompt: strings.ReplaceAll(prompt, "{source}", source), Confirming: n.privacyRules.pendingDelete == rule.Id,
		Disabled:        n.mutating || n.privacyDialog != nil,
		OnRequestRemove: func() { n.privacyRules.pendingDelete, n.privacyRules.actionFailed = rule.Id, false; c.Invalidate() },
		OnCancel:        func() { n.privacyRules.pendingDelete, n.privacyRules.actionFailed = "", false; c.Invalidate() },
		OnConfirm:       func() { n.removePrivacyRule(rule.Id) },
	})
}

func (n *nativeMain) removePrivacyRule(id string) {
	state := n.history.state
	if n.session == nil || state.ServiceInstanceId == "" || n.mutating || n.privacyRules.pendingDelete != id {
		return
	}
	n.mutating, n.privacyRules.actionFailed = true, false
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 8*time.Second)
		defer cancel()
		_, err := n.library.RemovePolicyRule(ctx, id, state.PrivacyEpoch)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.privacyRules.actionFailed = true
				return
			}
			n.reloadViewSettings()
			n.notice = n.text("privacy_feedback_done")
		})
	}()
}

func (n *nativeMain) drawPrivacyDetection(c *ui.Context, kit uikit.Kit) {
	n.drawSourceExclusions(c, kit)
	kit.SettingsDisclosure(c, n.text("privacy_protection_kinds"), &n.privacyKindsOpen, func() {
		const key = "app.privacy_protection_kinds"
		raw, stored := n.preferences[key]
		if !stored {
			raw = "phone,idcard,email,secret,password"
		}
		var selected []string
		for _, id := range strings.Split(raw, ",") {
			if id = strings.TrimSpace(id); id != "" {
				selected = append(selected, id)
			}
		}
		for _, kind := range []struct{ id, label string }{
			{"phone", n.text("privacy_kind_phone")},
			{"idcard", n.text("privacy_kind_idcard")},
			{"email", n.text("privacy_kind_email")},
			{"secret", n.text("privacy_kind_secret")},
			{"password", n.text("privacy_kind_password")},
		} {
			checked := slices.Contains(selected, kind.id)
			if kit.SettingsToggle(c, &checked, kind.label).Disabled(n.mutating).Changed() {
				next := slices.DeleteFunc(slices.Clone(selected), func(id string) bool { return id == kind.id })
				if checked {
					next = append(next, kind.id)
				}
				n.saveSetting(key, strings.Join(next, ","))
			}
		}
	})
	n.drawCustomPrivacyRules(c, kit)
}

func (n *nativeMain) drawCustomPrivacyRules(c *ui.Context, kit uikit.Kit) {
	label := n.text("privacy_protection_custom_rules")
	row := ui.Column(c.Key(customPrivacySetting)).ColumnSpan(-1).FillWidth().Gap(5)
	draft := &n.customPrivacyRules
	saved := n.privacyText(draft, customPrivacySetting)
	disabled := n.session == nil || draft.instance == "" || n.mutating
	row.Children(func() {
		kit.SettingsDisclosure(c, label, &n.privacyCustomOpen, func() {
			ui.Column(c).FillWidth().Padding(0, 0, 0, 30).Gap(5).Children(func() {
				previous := draft.value
				field := kit.TextArea(c, &draft.value, label).FillWidth().Height(80).ReadOnly(disabled).
					Placeholder(n.text("privacy_protection_custom_rules_placeholder")).Tooltip(n.text("privacy_rules_custom_patterns_hint"))
				n.observePrivacyText(draft, field, previous, saved, privacyTextDelay)
				n.drawPrivacyTextRetry(c, kit, draft, disabled)
			})
		})
	})
}
