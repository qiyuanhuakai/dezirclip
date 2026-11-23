package uikit

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

type MenuAction struct {
	Label    string
	Disabled bool
	Run      func()
	Children []MenuAction
}

type ContextMenu struct {
	actions func() []MenuAction
	x, y    float32
	root    int
	active  int
	child   int
	pointer ui.Point
}

func (m *ContextMenu) Active() bool { return m.actions != nil }
func (m *ContextMenu) Close()       { *m = ContextMenu{} }

func (m *ContextMenu) Open(anchor ui.Element, actions func() []MenuAction) {
	bounds := anchor.Bounds()
	x, y := float32(0), bounds.H
	if px, py, ok := anchor.PointerPosition(); ok {
		x, y = px, py
	}
	x, y = bounds.X+x, bounds.Y+y
	*m = ContextMenu{actions: actions, x: x, y: y, root: -1, active: -1, pointer: ui.Point{X: x, Y: y}}
}

// ContextMenu keeps the root and its submenu in the same overlay, in separate
// scroll regions. Space for the submenu is reserved when positioning the root.
func (k Kit) ContextMenu(c *ui.Context, menu *ContextMenu, dismiss string) {
	if !menu.Active() {
		return
	}
	actions := menu.actions()
	if menu.root < 0 || menu.root >= len(actions) || actions[menu.root].Disabled {
		menu.root = nextMenuAction(actions, -1, 1)
		menu.active = -1
	}
	if menu.active >= 0 && len(actions[menu.active].Children) == 0 {
		menu.active = -1
	}
	width, height := c.Size()
	rootStyle, childStyle := k.style("contextMenu"), k.style("contextSubmenu")
	rootItem, childItem := k.style("contextMenuItem"), k.style("contextSubmenuItem")
	hasChildren := slices.ContainsFunc(actions, func(action MenuAction) bool { return len(action.Children) > 0 })
	left, rootWidth, childLeft, childWidth := contextMenuColumns(menu.x, width, rootStyle.Width, childStyle.Width, hasChildren)
	rootHeight := menuHeight(rootStyle, rootItem, height, len(actions))
	top := max(6, min(menu.y, height-rootHeight-6))
	ui.Overlay(c, func() {
		overlay := ui.Column(c).Absolute().Top(0).Left(0).Right(0).Bottom(0).Focusable().Modal()
		overlay.Children(func() {
			backdrop := ui.ButtonBase(c).Label(dismiss).Absolute().Top(0).Left(0).Right(0).Bottom(0)
			if backdrop.Clicked() || backdrop.RightClicked() || overlay.Shortcut(0, ui.KeyEscape) {
				menu.Close()
				return
			}
			keyboard := menu.navigate(overlay, actions)
			if !menu.Active() {
				return
			}
			x, y, hasPointer := overlay.PointerPosition()
			pointer := ui.Point{X: x, Y: y}
			moved := !keyboard && hasPointer && pointer != menu.pointer
			if hasPointer {
				menu.pointer = pointer
			}

			childTop := top
			k.menuPanel(c, rootStyle, left, top, rootWidth, rootHeight, func() {
				for index, action := range actions {
					item := k.menuItem(c, menu, action, false, menu.root == index, menu.active == index)
					if !action.Disabled && (moved && item.Hovered() || len(action.Children) > 0 && item.Clicked()) {
						menu.root = index
						if len(action.Children) == 0 {
							menu.active = -1
						} else if menu.active != index {
							menu.openSubmenu(action)
						}
						c.Invalidate()
					}
					if keyboard && menu.root == index && (menu.active < 0 || menu.child < 0) {
						item.Focus().ScrollIntoView()
					}
					if menu.active == index {
						childTop = item.Bounds().Y
					}
				}
			})
			if !menu.Active() || menu.active < 0 {
				return
			}
			children := actions[menu.active].Children
			childHeight := menuHeight(childStyle, childItem, min(height, 332), len(children))
			childTop = max(6, min(childTop, height-childHeight-6))
			k.menuPanel(c, childStyle, childLeft, childTop, childWidth, childHeight, func() {
				for index, action := range children {
					item := k.menuItem(c, menu, action, true, menu.child == index, false)
					if keyboard && menu.child == index {
						item.Focus().ScrollIntoView()
					}
					if moved && !action.Disabled && item.Hovered() {
						menu.child = index
						c.Invalidate()
					}
				}
			})
		})
	})
}

func (m *ContextMenu) openSubmenu(action MenuAction) {
	m.active = m.root
	m.child = nextMenuAction(action.Children, -1, 1)
}

func (m *ContextMenu) navigate(keys ui.Element, root []MenuAction) bool {
	down := keys.Shortcut(0, ui.KeyDown)
	up := keys.Shortcut(0, ui.KeyUp)
	right := keys.Shortcut(0, ui.KeyRight)
	left := keys.Shortcut(0, ui.KeyLeft)
	enter := keys.Shortcut(0, ui.KeyEnter)
	if !down && !up && !right && !left && !enter {
		return false
	}
	level, selected := root, &m.root
	if m.active >= 0 {
		level, selected = root[m.active].Children, &m.child
	}
	switch {
	case down:
		*selected = nextMenuAction(level, *selected, 1)
	case up:
		*selected = nextMenuAction(level, *selected, -1)
	case left:
		m.active = -1
	case (right || enter) && m.active < 0 && m.root >= 0 && len(root[m.root].Children) != 0:
		m.openSubmenu(root[m.root])
	case enter && *selected >= 0 && *selected < len(level) && !level[*selected].Disabled:
		m.activate(level[*selected])
	}
	return true
}

func nextMenuAction(actions []MenuAction, index, direction int) int {
	if index < 0 && direction < 0 {
		index = 0
	}
	for range actions {
		index = (index + direction + len(actions)) % len(actions)
		if !actions[index].Disabled {
			return index
		}
	}
	return -1
}

func (m *ContextMenu) activate(action MenuAction) {
	if action.Run != nil {
		m.Close()
		action.Run()
	}
}

func contextMenuColumns(x, viewport, root, child float32, hasChildren bool) (left, rootWidth, childLeft, childWidth float32) {
	const edge, gap = 6, 4
	space := max(0, viewport-edge*2)
	if !hasChildren {
		root = min(root, space)
		return max(edge, min(x, viewport-root-edge)), root, 0, 0
	}
	if available := max(0, space-gap); root+child > available {
		root = min(root, available/2)
		child = min(child, available-root)
	}
	right := max(edge, min(x, viewport-edge-root-gap-child))
	left = max(edge+child+gap, min(x, viewport-edge-root))
	if x-right <= left-x {
		return right, root, right + root + gap, child
	}
	return left, root, left - child - gap, child
}

func menuHeight(surface, item theme.WidgetStyle, viewport float32, count int) float32 {
	return min(max(0, viewport-12), item.Height*float32(count)+surface.Padding[0]+surface.Padding[2]+2*surface.BorderWidth)
}

func (k Kit) menuPanel(c *ui.Context, style theme.WidgetStyle, x, y, width, height float32, content func()) {
	paint(ui.Column(c).Absolute().Left(x).Top(y).Size(width, height).Role(ui.RoleMenu), style).Children(func() {
		ui.Scroll(c).Fill().MinHeight(0).Children(content)
	})
}

func (k Kit) menuItem(c *ui.Context, menu *ContextMenu, action MenuAction, submenu, selected, expanded bool) ui.Element {
	widget := "contextMenuItem"
	if submenu {
		widget = "contextSubmenuItem"
	}
	style := k.style(widget)
	item := ui.ButtonBase(c).Label(action.Label).Tooltip(action.Label).FillWidth().Height(style.Height).
		Role(ui.RoleMenuItem).Disabled(action.Disabled).Cursor(actionCursor(action.Disabled))
	if len(action.Children) > 0 {
		item.Expanded(expanded)
	}
	if selected || item.FocusVisible() {
		style = k.style(widget + "Active")
	}
	if action.Disabled {
		style.Text = rgba(c.Theme().TextMuted)
	}
	paint(item.AlignItems(ui.Center).Gap(4), k.animatePaint(item, style))
	item.Children(func() {
		text(c, action.Label, style).Grow(1).MinWidth(0).SingleLine().PassThrough()
		if len(action.Children) > 0 {
			ui.Icon(c, chevronRight).Size(14, 14).Shrink(0).TextColor(styleColor(style.Text)).PassThrough()
		}
	})
	if len(action.Children) == 0 && item.Clicked() {
		menu.activate(action)
	}
	return item
}
