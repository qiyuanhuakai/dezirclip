package screenshot

import (
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"testing"
)

func TestPhysicalCursorIdentifiesFrozenMonitor(t *testing.T) {
	primary := protocol.ScreenImage{Width: 1920, Height: 1080}
	for _, side := range []protocol.ScreenImage{{X: 1920, Width: 3840, Height: 2160}, {X: -3840, Y: -360, Width: 3840, Height: 2160}} {
		// On the right 200% display, (2200,500) would map to (1100,250)
		// in logical units and overlap the 100% primary.
		target := captureTarget{point: protocol.ScreenPoint{X: side.X + 280, Y: side.Y + 500}}
		for _, order := range [][]protocol.ScreenImage{{primary, side}, {side, primary}} {
			for _, frame := range order {
				if target.matches(frame) != (frame.X == side.X) {
					t.Fatal("incorrect physical monitor", target, frame)
				}
			}
		}
	}
}
