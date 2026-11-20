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
	active  int
	child   int
}

func (m *ContextMenu) Active() bool { return m.actions != nil }
func (m *ContextMenu) Close()       { *m = ContextMenu{} }

func (m *ContextMenu) Open(anchor ui.Element, actions func() []MenuAction) {
	bounds := anchor.Bounds()
	x, y := float32(0), bounds.H
	if px, py, ok := anchor.PointerPosition(); ok {
		x, y = px, py
	}
	*m = ContextMenu{actions: actions, x: bounds.X + x, y: bounds.Y + y, active: -1}
}

// ContextMenu keeps the root and its submenu in the same overlay, in separate
// scroll regions. Space for the submenu is reserved when positioning the root.
func (k Kit) ContextMenu(c *ui.Context, menu *ContextMenu, dismiss string) {
	if !menu.Active() {
		return
	}
	actions := menu.actions()
	width, height := c.Size()
	rootStyle, childStyle := k.style("contextMenu"), k.style("contextSubmenu")
	rootItem, childItem := k.style("contextMenuItem"), k.style("contextSubmenuItem")
	hasChildren := slices.ContainsFunc(actions, func(action MenuAction) bool { return len(action.Children) > 0 })
	left, rootWidth, childLeft, childWidth := contextMenuColumns(menu.x, width, rootStyle.Width, childStyle.Width, hasChildren)
	rootHeight := menuHeight(rootStyle, rootItem, height, len(actions))
	top := max(6, min(menu.y, height-rootHeight-6))
	ui.Overlay(c, func() {
		backdrop := ui.ButtonBase(c).Label(dismiss).Absolute().Top(0).Left(0).Right(0).Bottom(0)
		if backdrop.Clicked() || backdrop.RightClicked() || backdrop.OverlayShortcut(0, ui.KeyEscape) {
			menu.Close()
			return
		}
		childTop := top
		k.menuPanel(c, rootStyle, left, top, rootWidth, rootHeight, func() {
			for index, action := range actions {
				item := k.menuItem(c, menu, action, false, menu.active == index)
				previous := ui.Local(item, "context-menu-hover", func() bool { return false })
				hovered := item.Hovered()
				if hovered && !*previous {
					menu.active = -1
					if len(action.Children) > 0 {
						menu.active = index
						menu.child = 0
					}
				}
				*previous = hovered
				if len(action.Children) > 0 && (item.Clicked() || item.Shortcut(0, ui.KeyRight)) {
					menu.active = index
					menu.child = 0
				}
				if menu.active == index {
					childTop = item.Bounds().Y
				}
			}
		})
		if menu.active < 0 || menu.active >= len(actions) || !menu.Active() {
			return
		}
		children := actions[menu.active].Children
		if len(children) == 0 {
			return
		}
		childHeight := menuHeight(childStyle, childItem, min(height, 332), len(children))
		childTop = max(6, min(childTop, height-childHeight-6))
		k.menuPanel(c, childStyle, childLeft, childTop, childWidth, childHeight, func() {
			for index, action := range children {
				item := k.menuItem(c, menu, action, true, menu.child == index)
				if item.Hovered() {
					menu.child = index
				}
			}
		})
	})
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
	paint(ui.Column(c).Absolute().Left(x).Top(y).Size(width, height), style).Children(func() {
		ui.Scroll(c).Fill().MinHeight(0).Children(content)
	})
}

func (k Kit) menuItem(c *ui.Context, menu *ContextMenu, action MenuAction, submenu, expanded bool) ui.Element {
	widget := "contextMenuItem"
	if submenu {
		widget = "contextSubmenuItem"
	}
	style := k.style(widget)
	item := ui.ButtonBase(c).Label(action.Label).Tooltip(action.Label).FillWidth().Height(style.Height).Disabled(action.Disabled)
	if expanded || item.Hovered() || item.FocusVisible() {
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
	if len(action.Children) == 0 && action.Run != nil && item.Clicked() {
		menu.Close()
		action.Run()
	}
	return item
}
