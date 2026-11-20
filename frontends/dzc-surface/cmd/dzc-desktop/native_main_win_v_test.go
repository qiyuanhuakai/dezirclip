package main

import (
	"context"
	"errors"
	"slices"
	"strconv"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type winVFixture struct {
	steps   []string
	failAt  string
	failed  error
	changed bool
}

func (f *winVFixture) step(ctx context.Context, operation string) error {
	f.steps = append(f.steps, operation)
	if operation == f.failAt {
		return f.failed
	}
	return ctx.Err()
}

func (f *winVFixture) BeginWinV(ctx context.Context, enabled bool) (protocol.WindowsSettingResult, error) {
	return protocol.WindowsSettingResult{Value: f.changed, Token: "pending"}, f.step(ctx, "begin="+strconv.FormatBool(enabled))
}

func (f *winVFixture) FinishWinV(ctx context.Context, token string, rollback bool) (bool, error) {
	return true, f.step(ctx, token+"/rollback="+strconv.FormatBool(rollback))
}

func (f *winVFixture) RestartExplorer(ctx context.Context) error {
	return f.step(ctx, "restart")
}

func TestWinVSwitchPreservesOtherBindingsAndRollsBack(t *testing.T) {
	failure := errors.New("simulated write failed")
	for _, test := range []struct {
		name, oldKeys, failAt string
		enable, changed       bool
		want                  []string
	}{
		{"enable", "Alt+C\nCtrl+Shift+V", "", true, true, []string{
			"begin=true", "restart", "hotkeys=Alt+C\nCtrl+Shift+V\nSuper+V", "pref=true", "pending/rollback=false",
		}},
		{"disable", "Alt+C\r\nWin+V\r\nWin+Shift+V", "", false, true, []string{
			"begin=false", "hotkeys=Alt+C\nWin+Shift+V", "restart", "pref=false", "pending/rollback=false",
		}},
		{"registration failure restores the system shortcut", "Alt+C", "hotkeys=Alt+C\nSuper+V", true, true, []string{
			"begin=true", "restart", "hotkeys=Alt+C\nSuper+V", "hotkeys=Alt+C", "pending/rollback=true", "restart",
		}},
		{"disable persistence failure restores original binding", "Alt+C\nWin+V", "pref=false", false, true, []string{
			"begin=false", "hotkeys=Alt+C", "restart", "pref=false", "pending/rollback=true", "restart",
			"hotkeys=Alt+C\nWin+V", "pref=true",
		}},
		{"claim saved alias when no restart is needed", "Alt+C\nWindows+V", "", true, false, []string{
			"begin=true", "hotkeys=Alt+C\nWindows+V", "pref=true", "pending/rollback=false",
		}},
		{"failed commit restores keys and preference", "Alt+C", "pending/rollback=false", true, true, []string{
			"begin=true", "restart", "hotkeys=Alt+C\nSuper+V", "pref=true", "pending/rollback=false",
			"hotkeys=Alt+C", "pending/rollback=true", "restart", "pref=false",
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			system := &winVFixture{changed: test.changed, failAt: test.failAt, failed: failure}
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			err := changeNativeWinV(ctx, system, test.enable, test.oldKeys, strconv.FormatBool(!test.enable),
				func(ctx context.Context, value string) error {
					err := system.step(ctx, "hotkeys="+value)
					if err != nil {
						cancel()
					}
					return err
				},
				func(ctx context.Context, value string) error { return system.step(ctx, "pref="+value) })
			if !slices.Equal(system.steps, test.want) {
				t.Fatalf("operations = %q, want %q", system.steps, test.want)
			}
			if test.failAt == "" && err != nil || test.failAt != "" && !errors.Is(err, failure) {
				t.Fatalf("unexpected Win+V result: %v", err)
			}
			if errors.Is(err, context.Canceled) {
				t.Fatal("rollback reused the canceled settings context")
			}
		})
	}
}
