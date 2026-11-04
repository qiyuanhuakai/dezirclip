package screenshot

import (
	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"golang.org/x/sys/windows"
	"unsafe"
)

var user32 = windows.NewLazySystemDLL("user32.dll")
var physicalCursor = user32.NewProc("GetPhysicalCursorPos")
var setWindowPos = user32.NewProc("SetWindowPos")
var getWindowRect = user32.NewProc("GetWindowRect")

func cursorTarget() (captureTarget, bool) {
	var p struct{ X, Y int32 }
	ok, _, _ := physicalCursor.Call(uintptr(unsafe.Pointer(&p)))
	return captureTarget{point: protocol.ScreenPoint{X: int64(p.X), Y: int64(p.Y)}}, ok != 0
}

// The Host's frozen rectangle is in physical desktop pixels. The MyGo UI
// thread is per-monitor DPI aware. Keep both operations in that coordinate
// space, including monitors to the left or above the primary.
func (t captureTarget) place(window *mygo.Window, frame protocol.ScreenImage) bool {
	hwnd := window.NativeHandle()
	const noZOrderOrActivate = 0x0004 | 0x0010
	for attempt := 0; attempt < 2; attempt++ {
		ok, _, _ := setWindowPos.Call(hwnd, 0, uintptr(frame.X), uintptr(frame.Y), uintptr(frame.Width), uintptr(frame.Height), noZOrderOrActivate)
		if ok == 0 {
			return false
		}
		var r struct{ Left, Top, Right, Bottom int32 }
		ok, _, _ = getWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&r)))
		if ok != 0 && int64(r.Left) == frame.X && int64(r.Top) == frame.Y &&
			int64(r.Right)-frame.X == int64(frame.Width) && int64(r.Bottom)-frame.Y == int64(frame.Height) {
			return true
		}
	}
	return false
}
