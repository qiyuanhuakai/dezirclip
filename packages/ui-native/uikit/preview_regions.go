package uikit

import "github.com/egoist/mygo/ui"

// WheelHotZone observes wheel input over its content and lets the native
// scroller continue processing it. A child with a custom input handler
// receives its own wheel events directly.
func WheelHotZone(c *ui.Context, onWheel func(ui.InputEvent), children func()) ui.Element {
	region := ui.Column(c).Grow(1).MinHeight(0).Clip()
	if onWheel != nil {
		region.HandleInput(func(event ui.InputEvent) bool {
			if event.Kind == ui.InputScroll {
				onWheel(event)
				c.Invalidate()
			}
			return false
		})
	}
	return region.Children(children)
}

type PreviewResizeDirection string

const (
	ResizeNorth     PreviewResizeDirection = "North"
	ResizeNorthEast PreviewResizeDirection = "NorthEast"
	ResizeEast      PreviewResizeDirection = "East"
	ResizeSouthEast PreviewResizeDirection = "SouthEast"
	ResizeSouth     PreviewResizeDirection = "South"
	ResizeSouthWest PreviewResizeDirection = "SouthWest"
	ResizeWest      PreviewResizeDirection = "West"
	ResizeNorthWest PreviewResizeDirection = "NorthWest"
)

// PreviewResizeHandles adds invisible pointer regions to the current panel.
// The caller starts its own resize operation when a region is pressed.
func PreviewResizeHandles(c *ui.Context, label string, onResize func(PreviewResizeDirection)) {
	add := func(direction PreviewResizeDirection, cursor ui.Cursor) ui.Element {
		return ui.Box(c).Absolute().Cursor(cursor).Tooltip(label).Role(ui.RoleNone).HandleInput(func(event ui.InputEvent) bool {
			if event.Kind != ui.InputPointerDown || event.Button != 0 || onResize == nil {
				return false
			}
			onResize(direction)
			return true
		})
	}
	add(ResizeNorth, ui.CursorResizeN).Top(0).Left(10).Right(10).Height(6)
	add(ResizeEast, ui.CursorResizeE).Right(0).Top(10).Bottom(10).Width(6)
	add(ResizeSouth, ui.CursorResizeS).Bottom(0).Left(10).Right(10).Height(6)
	add(ResizeWest, ui.CursorResizeW).Left(0).Top(10).Bottom(10).Width(6)
	add(ResizeNorthEast, ui.CursorResizeNESW).Top(0).Right(0).Size(10, 10)
	add(ResizeSouthEast, ui.CursorResizeNWSE).Bottom(0).Right(0).Size(10, 10)
	add(ResizeSouthWest, ui.CursorResizeNESW).Bottom(0).Left(0).Size(10, 10)
	add(ResizeNorthWest, ui.CursorResizeNWSE).Top(0).Left(0).Size(10, 10)
}
