package uikit

import "github.com/egoist/mygo/ui"

type MenuAction struct {
	Label    string
	Disabled bool
	Run      func()
	Children []MenuAction
}

type ContextMenu struct {
	actions func() []MenuAction
	x, y    float32
}

func (m *ContextMenu) Active() bool { return m.actions != nil }
func (m *ContextMenu) Close()       { *m = ContextMenu{} }

func (m *ContextMenu) Open(anchor ui.Element, actions func() []MenuAction) {
	bounds := anchor.Bounds()
	x, y := float32(0), bounds.H
	if px, py, ok := anchor.PointerPosition(); ok {
		x, y = px, py
	}
	*m = ContextMenu{actions: actions, x: bounds.X + x, y: bounds.Y + y}
}

// ContextMenu paints all levels on the same surface and keeps the menu inside
// the viewport. Submenus expand below their parent, including at minimum size.
func (k Kit) ContextMenu(c *ui.Context, menu *ContextMenu, dismiss string) {
	if !menu.Active() {
		return
	}
	actions := menu.actions()
	width, height := c.Size()
	menuWidth := min(width-12, 246)
	style := k.style("selectMenu")
	style.Opacity = 1
	if style.Fill[3] < 235 {
		style.Fill = rgba(c.Theme().Surface)
	}
	ui.Overlay(c, func() {
		backdrop := ui.ButtonBase(c).Label(dismiss).Absolute().Top(0).Left(0).Right(0).Bottom(0)
		if backdrop.Clicked() || backdrop.RightClicked() || backdrop.OverlayShortcut(0, ui.KeyEscape) {
			menu.Close()
			return
		}
		panel := paint(ui.Column(c).Absolute().Left(max(6, min(menu.x, width-menuWidth-6))).
			Top(max(6, min(menu.y, height-float32(len(actions))*32-22))).
			Width(menuWidth).MaxHeight(height-12), style)
		expanded := ui.Local(panel, "deziro-menu-section", func() int { return -1 })
		panel.Children(func() {
			ui.Scroll(c).FillWidth().MaxHeight(height - 24).Children(func() {
				for index, action := range actions {
					if len(action.Children) == 0 {
						k.menuItem(c, menu, action, false, nil)
						continue
					}
					open := *expanded == index
					k.menuItem(c, menu, action, open, func() { *expanded = index })
					if open {
						ui.Column(c).PaddingStart(12).Children(func() {
							for _, child := range action.Children {
								k.menuItem(c, menu, child, false, nil)
							}
						})
					}
				}
			})
		})
	})
}

func (k Kit) menuItem(c *ui.Context, menu *ContextMenu, action MenuAction, expanded bool, toggle func()) {
	style := k.style("selectOption")
	if expanded {
		style = k.style("selectSelected")
	}
	if action.Disabled {
		style = k.style("selectOptionDisabled")
	}
	item := ui.ButtonBase(c).Label(action.Label).FillWidth().Height(style.Height).Disabled(action.Disabled)
	paint(item, k.interactive(c, item, style, expanded))
	item.Children(func() {
		text(c, action.Label, style).Grow(1).SingleLine().PassThrough()
		if toggle != nil {
			arrow := chevronRight
			if expanded {
				arrow = chevronDown
			}
			ui.Icon(c, arrow).Size(14, 14).TextColor(styleColor(style.Text)).PassThrough()
		}
	})
	hovered := item.Hovered()
	previous := ui.Local(item, "deziro-menu-hover", func() bool { return false })
	entered := hovered && !*previous
	*previous = hovered
	if item.Clicked() || toggle != nil && !action.Disabled && entered && !expanded {
		if toggle != nil {
			toggle()
		} else if action.Run != nil {
			menu.Close()
			action.Run()
		}
	}
}
