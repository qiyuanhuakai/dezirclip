package screenshot

import (
	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"math"
)

func cursorTarget() (captureTarget, bool) {
	cursor := mygo.Screen.CursorScreenPoint()
	display := mygo.Screen.DisplayNearestPoint(cursor)
	scale := display.ScaleFactor
	return captureTarget{point: protocol.ScreenPoint{X: int64(math.Round(float64(cursor.X) * scale)), Y: int64(math.Round(float64(cursor.Y) * scale))}, bounds: display.Bounds}, scale > 0
}

func (t captureTarget) place(window *mygo.Window, _ protocol.ScreenImage) bool {
	window.SetBounds(t.bounds)
	return true
}
