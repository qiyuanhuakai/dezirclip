package main

import (
	"errors"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestNativeShortcutRecorderUsesResidentChordNames(t *testing.T) {
	for _, test := range []struct {
		key  ui.Key
		mods ui.Modifiers
		want string
	}{
		{ui.KeyJ, ui.Shift | ui.Ctrl | ui.Alt, "Ctrl+Alt+Shift+J"},
		{ui.KeyF12, ui.Super, "Super+F12"},
		{ui.Key0, ui.Alt, "Alt+0"},
		{ui.KeyLeft, ui.Ctrl, "Ctrl+Left"},
		{ui.KeyEscape, 0, ""},
		{ui.KeyEscape, ui.Ctrl, "Ctrl+Escape"},
		{ui.KeyEqual, ui.Shift, "Shift+Equal"},
		{ui.KeyBackslash, ui.Ctrl, "Ctrl+Backslash"},
		{ui.KeySemicolon, ui.Alt | ui.Shift, "Alt+Shift+Semicolon"},
		{ui.KeyUnknown, ui.Ctrl, ""},
	} {
		if actual := nativeShortcutChord(test.key, test.mods); actual != test.want {
			t.Fatalf("key %v with modifiers %v gave %q, want %q", test.key, test.mods, actual, test.want)
		}
	}
	if chords := nativeMainChords("control+j\r\nMButton\n  Shift+F3\n"); !slices.Equal(chords, []string{"Ctrl+J", "MouseMiddle", "Shift+F3"}) {
		t.Fatalf("unexpected bindings after reading existing settings: %v", chords)
	}
	if actual := typedShortcutChord("  control+alt+f24 "); actual != "Ctrl+Alt+F24" {
		t.Fatalf("typed advanced chord = %q", actual)
	}
	if actual := typedShortcutChord("Alt+C\nAlt+F"); actual != "" {
		t.Fatalf("single shortcut field accepted multiple chords: %q", actual)
	}
}

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

func TestExtendedShortcutNamesKeepKeypadSeparate(t *testing.T) {
	for _, tt := range []struct{ raw, canonical, accelerator string }{
		{"Ctrl+Num1", "Ctrl+Numpad1", "Ctrl+Num1"},
		{"Ctrl+1", "Ctrl+1", "Ctrl+1"},
		{"Shift+NumPlus", "Shift+NumpadAdd", "Shift+NumAdd"},
		{"Shift+Plus", "Shift+Equal", "Shift+="},
		{"Ctrl+;", "Ctrl+Semicolon", "Ctrl+;"},
		{"Alt+MediaNextTrack", "Alt+MediaTrackNext", "Alt+MediaNextTrack"},
		{"Ctrl+PrintScreen", "Ctrl+PrintScreen", "Ctrl+PrintScreen"},
		{"Alt+OEM102", "Alt+IntlBackslash", "Alt+IntlBackslash"},
		{"Ctrl+NumEqual", "Ctrl+NumpadEqual", "Ctrl+NumpadEqual"},
		{"Ctrl+NumpadComma", "Ctrl+NumpadSeparator", "Ctrl+NumpadSeparator"},
	} {
		if got := mainShortcutName(tt.raw); got != tt.canonical {
			t.Errorf("%q canonicalizes to %q, want %q", tt.raw, got, tt.canonical)
		}
		if got := mygoShortcutName(tt.raw); got != tt.accelerator {
			t.Errorf("%q registers as %q, want %q", tt.raw, got, tt.accelerator)
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
