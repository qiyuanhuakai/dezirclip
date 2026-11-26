package main

import (
	"context"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

func TestNativePrivacyRuleConfirmationStaysInItsCard(t *testing.T) {
	source := "Example Editor"
	const id = "a032005c-test"
	n := nativeMain{
		preferences: map[string]string{"app.language": "en"},
		privacyRules: nativePrivacyRules{
			started: true,
			rules: []protocol.PolicyRuleValue{
				{Id: id, Kind: "ignore_source", SourceApp: &source, CreatedAtMs: 1795040000000},
				{Id: "ae82023b-test", Kind: "exact_exception", SourceApp: &source},
			},
		},
	}
	view := ui.NewTester(n.drawPrivacyRules, 740, 420)
	removeLabel := n.text("privacy_rule_remove")
	first, firstOK := view.Find(removeLabel)
	second, secondOK := view.Find(n.text("privacy_rule_revoke_exception"))
	if !firstOK || !secondOK || first.X+first.W > second.X {
		t.Fatalf("side-by-side rule actions overlap: first=%+v second=%+v", first, second)
	}
	if err := view.Click(removeLabel); err != nil {
		t.Fatal(err)
	}
	if n.privacyRules.pendingDelete != id || n.privacyDialog != nil {
		t.Fatal("confirmation should open in the selected rule")
	}
	if _, ok := view.Find(n.text("privacy_rule_delete_title")); !ok {
		t.Fatal("inline confirmation is missing its accessible group label")
	}
	if err := view.Click(n.text("cancel")); err != nil {
		t.Fatal(err)
	}
	if n.privacyRules.pendingDelete != "" || len(n.privacyRules.rules) != 2 {
		t.Fatal("cancel should leave the rule in the list")
	}
}

func TestNativePrivacyExclusionsStayInlineAndCustomRulesExpand(t *testing.T) {
	n := nativeMain{preferences: map[string]string{"app.language": "en"}}
	view := ui.NewTester(func(c *ui.Context) {
		n.drawPrivacyDetection(c, n.mainKit())
	}, 390, 430)
	if _, ok := view.Find(n.text("excluded_source_apps")); !ok {
		t.Fatal("application exclusions should have a direct input")
	}
	if n.privacyCustomOpen {
		t.Fatal("custom detection patterns should start collapsed")
	}
	if err := view.Click(n.text("privacy_protection_custom_rules")); err != nil || !n.privacyCustomOpen {
		t.Fatalf("custom pattern disclosure did not open: %v", err)
	}
	if n.privacyDialog != nil {
		t.Fatal("opening custom detection patterns should stay in settings")
	}
}

func TestNativePrivacySettingsShowHelpAndTimedSession(t *testing.T) {
	n := nativeMain{
		showSettings: true, settingsSection: "privacy", private: true,
		preferences: map[string]string{"app.language": "en"},
	}
	view := ui.NewTester(n.view, 820, 680)
	shows := func(text string) bool { return strings.Contains(strings.Join(view.Texts(), "\n"), text) }
	until := n.text("privacy_session_until")
	if shows(until) {
		t.Fatal("an indefinite session displays a time limit")
	}
	expiry := time.Now().Add(15 * time.Minute).UnixMilli()
	n.privateUntil = &expiry
	view.Frame()
	if !shows(until) {
		t.Fatal("missing the end of the timed session")
	}
	for _, field := range []struct{ label, description string }{
		{"privacy_session_title", "privacy_session_hint"},
		{"privacy_protection", "privacy_protection_hint"},
	} {
		hint := n.text(field.description)
		if shows(hint) {
			t.Fatalf("%s help starts expanded", field.label)
		}
		if err := view.Click(n.settingsHelpLabel(n.text(field.label))); err != nil || !shows(hint) || !n.private {
			t.Fatalf("%s help did not expand independently: %v", field.label, err)
		}
	}
	n.private = false
	view.Frame()
	if shows(until) {
		t.Fatal("an inactive session displays an old time limit")
	}
}

func TestNativePrivacyExclusionDraftSurvivesSettingsCloseWhileBusy(t *testing.T) {
	n := nativeMain{
		session: context.Background(), showSettings: true, settingsSection: "privacy",
		privacyRules: nativePrivacyRules{started: true},
		preferences:  map[string]string{"app.language": "en"},
	}
	n.history.state.ServiceInstanceId = "profile-a"
	view := ui.NewTester(n.view, 820, 640)
	label := n.text("excluded_source_apps")
	if err := view.Click(n.settingsHelpLabel(label)); err != nil {
		t.Fatal(err)
	}
	shown := strings.Join(view.Texts(), "\n")
	for _, key := range []string{"excluded_source_apps_hint", "privacy_rules_legacy_exclusions_hint"} {
		if !strings.Contains(shown, n.text(key)) {
			t.Fatalf("expanded exclusions are missing %s", key)
		}
	}
	if err := view.Click(n.settingsHelpLabel(label)); err != nil {
		t.Fatal(err)
	}
	labelBox, ok := view.Find(label)
	if !ok {
		t.Fatal("missing application exclusion label")
	}
	view.ClickAt(labelBox.X+12, labelBox.Y+labelBox.H+30)
	view.Type("Example Editor.exe")
	if draft := n.sourceExclusions; !draft.editing || !draft.dirty || draft.value != "Example Editor.exe" {
		t.Fatalf("typed exclusions were not held in an active draft: %+v", draft)
	}
	if view.HasText(n.text("common_save")) {
		t.Fatal("application exclusions should save without a separate button")
	}
	n.mutating = true
	if err := view.Click(n.text("common_close")); err != nil {
		t.Fatal(err)
	}
	if draft := n.sourceExclusions; n.showSettings || draft.editing || !draft.dirty || draft.value != "Example Editor.exe" {
		t.Fatalf("closing settings should queue the inactive draft: %+v", draft)
	}
	n.history.state.ServiceInstanceId = "profile-b"
	n.preferences[sourceExclusionsSetting] = "Other Editor"
	n.openSettings()
	n.settingsSection = "privacy"
	view.Frame()
	if draft := n.sourceExclusions; draft.dirty || draft.value != "Other Editor" || draft.instance != "profile-b" {
		t.Fatalf("a different profile inherited the previous draft: %+v", draft)
	}
}

func TestNativeCustomPrivacyPatternsUseAnInlineAutosaveDraft(t *testing.T) {
	n := nativeMain{session: context.Background(), preferences: map[string]string{"app.language": "en"}}
	n.history.state.ServiceInstanceId = "profile-a"
	view := ui.NewTester(func(c *ui.Context) { n.drawCustomPrivacyRules(c, n.mainKit()) }, 390, 245)
	label := n.text("privacy_protection_custom_rules")
	if err := view.Click(label); err != nil {
		t.Fatal(err)
	}
	labelBounds, ok := view.Find(label)
	if !ok {
		t.Fatal("missing privacy rules disclosure")
	}
	view.ClickAt(labelBounds.X+60, labelBounds.Y+labelBounds.H+40)
	before := time.Now()
	view.Type("example-[0-9]+")
	draft := &n.customPrivacyRules
	if !draft.dirty || draft.value != "example-[0-9]+" || !draft.ready.After(before) {
		t.Fatalf("custom pattern did not request a delayed save: %+v", draft)
	}
	if view.HasText(n.text("common_save")) {
		t.Fatal("custom patterns should save without a separate button")
	}
}

func TestNativeClipboardMenuKeepsOnlyPrivacyAndTextProcessingInSubmenus(t *testing.T) {
	n := nativeMain{preferences: map[string]string{"app.language": "en"}}
	assertMenu := func(kind string, pinned bool, requiredRoot, requiredSubmenu []string) {
		t.Helper()
		actions := n.entryMenuActions(protocol.DesktopEntry{Id: 17, ContentType: kind, IsPinned: pinned, Tags: []string{}}, 0, ui.Rect{})
		var root, submenu []string
		for _, action := range actions {
			if len(action.Children) == 0 {
				root = append(root, action.Label)
			} else {
				submenu = append(submenu, action.Label)
			}
		}
		for _, label := range requiredRoot {
			if !slices.Contains(root, label) {
				t.Errorf("%s: %q must be a direct menu action, got %v", kind, label, root)
			}
		}
		if !slices.Equal(submenu, requiredSubmenu) {
			t.Errorf("%s: submenus = %v, want %v", kind, submenu, requiredSubmenu)
		}
	}
	privacy := n.text("privacy_management")
	assertMenu("text", false, []string{n.text("common_copy"), n.text("native_edit_tags"), n.text("native_qr_code"),
		n.text("delete", "Delete entry"), n.text("pin"), n.shareLabel()}, []string{n.text("native_transformations"), privacy})
	assertMenu("rich_text", true, []string{n.text("native_paste_formatted"), n.text("unpin")}, []string{n.text("native_transformations"), privacy})
	assertMenu("image", false, []string{n.text("native_ocr_recognize"), n.text("native_copy_base64")}, []string{privacy})
	entry := protocol.DesktopEntry{Id: 18, ContentType: "text"}
	protected := n.entryMenuActions(entry, 0, ui.Rect{})
	qr := slices.IndexFunc(protected, func(action uikit.MenuAction) bool {
		return action.Label == n.text("native_qr_code")
	})
	if qr < 0 || !protected[qr].Disabled || len(protected[qr].Children) > 0 {
		t.Fatalf("QR for an unrevealed entry must be a disabled root action: %v", protected)
	}
	transform := func(action uikit.MenuAction) bool { return action.Label == n.text("native_transformations") }
	if slices.ContainsFunc(protected, transform) {
		t.Fatal("the menu exposed text processing before revealing the protected entry")
	}
	n.protectedRows = map[nativeCardKey]*nativeProtectedRow{{id: entry.Id}: {body: &nativeProtectedBody{}}}
	revealed := n.entryMenuActions(entry, 0, ui.Rect{})
	index := slices.IndexFunc(revealed, transform)
	if index < 0 || !revealed[index].Disabled || len(revealed[index].Children) == 0 {
		t.Fatalf("text processing must remain a submenu and require the window to be focused: %v", revealed)
	}
}
