package main

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
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
