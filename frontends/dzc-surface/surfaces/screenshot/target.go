package screenshot

import (
	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type captureTarget struct {
	point  protocol.ScreenPoint
	bounds mygo.Rectangle
}

// Reject a fallback frame if the cursor's monitor disappeared after the shortcut.
func (t captureTarget) matches(frame protocol.ScreenImage) bool {
	return t.point.X >= frame.X && t.point.X-frame.X < int64(frame.Width) &&
		t.point.Y >= frame.Y && t.point.Y-frame.Y < int64(frame.Height)
}
