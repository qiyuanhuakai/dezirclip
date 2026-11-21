package uikit

import "github.com/egoist/mygo/ui"

// PreviewText wraps plain clipboard content and allows the preview to scroll.
func PreviewText(c *ui.Context, value string) ui.Element {
	body := ui.Scroll(c).FillWidth().Grow(1).MinHeight(0)
	body.Children(func() {
		ui.Text(c, value).FillWidth().LineHeight(1.5).Wrap()
	})
	return body
}
