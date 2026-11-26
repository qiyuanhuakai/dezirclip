package main

import "github.com/egoist/mygo"

// Visibility includes pinned previews, native panels and the main window in
// the separate desktop process.
func installMemoryPolicy(extraVisible func() bool) func() {
	background := false
	refresh := func() {
		visible := extraVisible()
		for _, window := range mygo.Windows() {
			visible = visible || (!window.IsDestroyed() && window.IsVisible())
		}
		if next := !visible; next != background {
			background = next
			setProcessMemoryBackground(next)
		}
	}
	mygo.App.OnWindowCreated(func(window *mygo.Window) {
		window.OnShow(refresh)
		window.OnHide(refresh)
		window.OnClosed(refresh)
	})
	return refresh
}
