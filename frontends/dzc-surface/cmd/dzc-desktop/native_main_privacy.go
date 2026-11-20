package main

import (
	"context"
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
}

type nativePrivacyDialog struct {
	open, loading, failed bool
	cancel                context.CancelFunc
	params                protocol.DesktopPolicyParams
	source, hint          string
	prompt                string
}

type nativePrivacyEditor struct {
	open, failed, focus     bool
	key, label, hint, draft string
}

type nativePrivacyFeedbackAction struct {
	id, label, prompt string
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
	title := n.text("privacy_feedback_title")
	if dialog.params.RuleId != nil {
		title = n.text("privacy_rule_delete_title")
	}
	kit.Dialog(c, &dialog.open, title, "", n.mutating, func() {
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
		(dialog.params.RuleId == nil && (dialog.params.Id == nil || dialog.params.Action == nil)) {
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
		var err error
		if params.RuleId != nil {
			_, err = n.library.RemovePolicyRule(ctx, *params.RuleId, params.ExpectedEpoch)
		} else {
			_, err = n.library.PolicyFeedback(ctx, params)
		}
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
	ui.Text(c, n.text("privacy_rules_title")).FontWeight(600)
	if !n.privacyRules.started {
		n.loadPrivacyRules()
	}
	switch {
	case !n.privacyRules.started || n.privacyRules.loading:
		ui.Text(c, n.text("privacy_rules_loading")).TextColor(c.Theme().TextMuted).Wrap()
	case n.privacyRules.failed:
		ui.Text(c, n.text("privacy_rules_load_failed")).TextColor(c.Theme().Danger).Wrap()
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_retry"), Disabled: n.mutating}).Clicked() {
			n.loadPrivacyRules()
		}
	case len(n.privacyRules.rules) == 0:
		ui.Text(c, n.text("privacy_rules_empty")).TextColor(c.Theme().TextMuted).Wrap()
	default:
		for _, rule := range n.privacyRules.rules {
			n.drawPrivacyRule(c, rule)
		}
	}
}

func (n *nativeMain) drawPrivacyRule(c *ui.Context, rule protocol.PolicyRuleValue) {
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
	ui.Column(c).FillWidth().Gap(5).Padding(8).Radius(c.Theme().Radius).
		Border(1, c.Theme().Border).Children(func() {
		ui.Text(c, name).FontWeight(600).Wrap()
		ui.Text(c, source).Wrap()
		ui.Text(c, scope).TextColor(c.Theme().TextMuted).Wrap()
		stamp := time.UnixMilli(rule.CreatedAtMs).Local().Format("2006-01-02 15:04")
		ui.Text(c, n.text("privacy_rule_identifier")+" "+rule.Id[:min(8, len(rule.Id))]+"     "+
			n.text("privacy_rule_created")+" "+stamp).TextColor(c.Theme().TextMuted).Wrap()
		label, prompt := n.text("privacy_rule_remove"), n.text("privacy_rule_delete_source_confirm")
		if rule.Kind == "exact_exception" {
			label, prompt = n.text("privacy_rule_revoke_exception"), n.text("privacy_rule_delete_exact_confirm")
		}
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: label, Disabled: n.mutating || n.privacyDialog != nil}).Clicked() {
			state := n.history.state
			n.privacyDialog = &nativePrivacyDialog{
				open: true, prompt: strings.ReplaceAll(prompt, "{source}", source),
				params: protocol.DesktopPolicyParams{
					ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, RuleId: &rule.Id,
				},
			}
		}
	})
}

func (n *nativeMain) drawPrivacyDetection(c *ui.Context, kit uikit.Kit) {
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
			if kit.Checkbox(c, &checked, kind.label).Disabled(n.mutating).Changed() {
				next := slices.DeleteFunc(slices.Clone(selected), func(id string) bool { return id == kind.id })
				if checked {
					next = append(next, kind.id)
				}
				n.saveSetting(key, strings.Join(next, ","))
			}
		}
	})
	for _, field := range []struct{ key, label, hint string }{
		{"app.excluded_source_apps", n.text("excluded_source_apps"), n.text("excluded_source_apps_hint")},
		{"app.privacy_protection_custom_rules", n.text("privacy_protection_custom_rules"), n.text("privacy_rules_custom_patterns_hint")},
	} {
		if n.mainKit().Button(c, uikit.ButtonOptions{
			Label: field.label, Disabled: n.session == nil || n.history.state.ServiceInstanceId == "" || n.mutating || n.privacyEditor != nil,
		}).Clicked() {
			n.privacyEditor = &nativePrivacyEditor{open: true, focus: true, key: field.key, label: field.label, hint: field.hint, draft: n.preferences[field.key]}
		}
	}
}

func (n *nativeMain) privacyEditorView(c *ui.Context, kit uikit.Kit, form *nativePrivacyEditor) {
	kit.Dialog(c, &form.open, form.label, "", n.mutating, func() {
		_, height := c.Size()
		ui.Scroll(c).FillWidth().MaxHeight(max(72, height-146)).Children(func() {
			ui.Column(c).FillWidth().Gap(6).Children(func() {
				ui.Text(c, form.hint).TextColor(c.Theme().TextMuted).Wrap()
				field := kit.TextArea(c, &form.draft, form.label).FillWidth().Height(100).ReadOnly(n.mutating)
				if form.focus {
					field.Focus()
					form.focus = false
				}
				if form.failed {
					ui.Text(c, n.text("privacy_rules_save_failed")).TextColor(c.Theme().Danger).Wrap()
				}
			})
		})
		ui.Row(c).Gap(6).Justify(ui.End).Children(func() {
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_cancel"), Disabled: n.mutating}).Clicked() {
				form.open = false
			}
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_save"), Primary: true, Disabled: n.mutating}).Clicked() {
				n.savePrivacyDraft(form)
			}
		})
	})
	if !form.open && n.privacyEditor == form {
		n.privacyEditor = nil
	}
}

func (n *nativeMain) savePrivacyDraft(form *nativePrivacyEditor) {
	state := n.history.state
	if n.session == nil || state.ServiceInstanceId == "" || n.mutating || n.privacyEditor != form {
		return
	}
	if form.draft == n.preferences[form.key] {
		form.open = false
		return
	}
	n.mutating, form.failed = true, false
	session, key, draft := n.session, form.key, form.draft
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		err := n.library.SetSetting(ctx, key, draft, state.PrivacyEpoch)
		n.update(session, func() {
			n.mutating = false
			if n.privacyEditor != form {
				return
			}
			if err != nil {
				form.failed = true
				return
			}
			n.prepare(n.resume)
		})
	}()
}
