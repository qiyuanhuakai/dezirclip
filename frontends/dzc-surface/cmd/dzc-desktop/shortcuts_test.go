package main

import (
	"errors"
	"testing"
)

func TestCanonicalMainShortcutContinuousEditsAndRollback(t *testing.T) {
	registered, live := map[string]bool{}, map[string]bool{}
	register := func(key string) error {
		if live[key] {
			t.Fatalf("duplicate native claim: %s", key)
		}
		live[key] = true
		return nil
	}
	unregister := func(key string) { delete(live, key) }
	for _, raw := range []string{"Ctrl+Shift+Alt+K", "Ctrl+Alt+Shift+K\nAlt+J", "shift+control+alt+k\nAlt+J"} {
		if err := changeMainHotkeys(registered, raw, register, unregister, func() error { return nil }); err != nil {
			t.Fatal(err)
		}
	}
	failure := errors.New("save failed")
	if err := changeMainHotkeys(registered, "Control+Shift+Alt+K\nAlt+F9", register, unregister, func() error { return failure }); !errors.Is(err, failure) {
		t.Fatal(err)
	}
	if len(live) != 2 || !live["Ctrl+Alt+Shift+K"] || !live["Alt+J"] {
		t.Fatal(live)
	}
	for raw, canonical := range map[string]string{"win+control+return": "Ctrl+Super+Enter", "shift+esc": "Shift+Escape", "alt+ArrowLeft": "Alt+Left", "meta+del": "Super+Delete", "CTRL+f24": "Ctrl+F24"} {
		if got := mainShortcutName(raw); got != canonical {
			t.Fatalf("%s: %s != %s", raw, got, canonical)
		}
	}
}

func TestMainShortcutAliasesShareOneNativeClaim(t *testing.T) {
	registered := map[string]bool{}
	claims := 0
	err := changeMainHotkeys(registered, "MButton\nmousemiddle", func(key string) error {
		if key != "MouseMiddle" {
			t.Fatal(key)
		}
		claims++
		return nil
	}, func(string) { t.Fatal("unexpected release") }, func() error { return nil })
	if err != nil || claims != 1 || !registered["MouseMiddle"] {
		t.Fatalf("claims=%d err=%v", claims, err)
	}
}

func TestMainHotkeysPreserveOldClaimsOnRegistrationOrSaveFailure(t *testing.T) {
	for _, failRegister := range []bool{true, false} {
		registered := map[string]bool{"Alt+C": true}
		live := map[string]bool{"Alt+C": true}
		calls, saves := 0, 0
		failure := errors.New("shortcut change failed")
		err := changeMainHotkeys(registered, "Ctrl+Alt+F21\nCtrl+Alt+F22", func(key string) error {
			calls++
			if failRegister && calls == 2 {
				return failure
			}
			live[key] = true
			return nil
		}, func(key string) { delete(live, key) }, func() error { saves++; return failure })
		if !errors.Is(err, failure) || len(registered) != 1 || !registered["Alt+C"] || len(live) != 1 || !live["Alt+C"] {
			t.Fatalf("failed change lost original claims: registered=%v live=%v err=%v", registered, live, err)
		}
		if failRegister && saves != 0 {
			t.Fatal("saved settings after failed native registration")
		}
	}
}

func TestMainHotkeysReleaseRemovedClaimsOnlyAfterSaving(t *testing.T) {
	registered := map[string]bool{"Alt+C": true}
	live := map[string]bool{"Alt+C": true}
	saved := false
	err := changeMainHotkeys(registered, "Ctrl+Alt+F22", func(key string) error { live[key] = true; return nil },
		func(key string) {
			if !saved {
				t.Fatal("released old binding before saving")
			}
			delete(live, key)
		},
		func() error {
			if !live["Alt+C"] || !live["Ctrl+Alt+F22"] {
				t.Fatal("claims not held during save")
			}
			saved = true
			return nil
		})
	if err != nil || len(registered) != 1 || !registered["Ctrl+Alt+F22"] || len(live) != 1 || !live["Ctrl+Alt+F22"] {
		t.Fatalf("successful change: registered=%v live=%v err=%v", registered, live, err)
	}
	err = changeMainHotkeys(registered, "", func(string) error { t.Fatal("clear registered a shortcut"); return nil },
		func(key string) { delete(live, key) }, func() error { return nil })
	if err != nil || len(registered) != 0 || len(live) != 0 {
		t.Fatalf("clear failed: %v", err)
	}
}
