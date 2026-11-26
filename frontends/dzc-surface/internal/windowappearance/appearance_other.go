//go:build !windows

package windowappearance

import "github.com/egoist/mygo"

func Apply(_ *mygo.Window, _, _ string, _ bool) error { return nil }
