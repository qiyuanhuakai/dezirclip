package main

import (
	"time"

	"github.com/egoist/mygo"
)

// Visibility includes pinned previews, native panels and the main window in
// the separate desktop process.
func installMemoryPolicy(extraVisible func() bool) func() {
	background := false
	var pendingTrim *time.Timer
	var generation uint64
	refresh := func() {
		visible := extraVisible()
		for _, window := range mygo.Windows() {
			visible = visible || (!window.IsDestroyed() && window.IsVisible())
		}
		if next := !visible; next != background {
			background = next
			setProcessMemoryBackground(next)
			generation++
			if pendingTrim != nil {
				pendingTrim.Stop()
				pendingTrim = nil
			}
			if next && hiddenWorkingSetTrimDelay > 0 {
				hidden := generation
				pendingTrim = time.AfterFunc(hiddenWorkingSetTrimDelay, func() {
					mygo.RunOnMain(func() {
						if hidden == generation {
							pendingTrim = nil
							trimHiddenWorkingSet()
						}
					})
				})
			}
		}
	}
	mygo.App.OnWindowCreated(func(window *mygo.Window) {
		window.OnShow(refresh)
		window.OnHide(refresh)
		window.OnClosed(refresh)
	})
	return refresh
}
