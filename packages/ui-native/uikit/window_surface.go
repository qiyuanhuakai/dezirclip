package uikit

import "github.com/egoist/mygo/ui"

// WindowContent paints the material behind the main panel of a desktop window.
func (k Kit) WindowContent(c *ui.Context, body func()) ui.Element {
	panel := ui.Column(c).FillWidth().Grow(1).MinHeight(0).
		Background(styleColor(k.style("windowContent").Fill))
	panel.Children(body)
	return panel
}
