package main

import (
	"testing"

	"github.com/egoist/mygo"
)

func TestMainWindowPlacement(t *testing.T) {
	work := mygo.Rectangle{X: -1600, Y: -200, Width: 1600, Height: 860}
	for _, test := range []struct {
		cursor        mygo.Point
		width, height int
		want          mygo.Point
	}{
		{mygo.Point{X: -800, Y: -100}, 660, 500, mygo.Point{X: -1130, Y: -88}},
		{mygo.Point{X: -5, Y: 640}, 660, 500, mygo.Point{X: -665, Y: 128}},
		{mygo.Point{X: -1599, Y: 200}, 660, 620, mygo.Point{X: -1595, Y: 35}},
		{mygo.Point{X: -800, Y: 200}, 2000, 1000, mygo.Point{X: -1600, Y: -200}},
	} {
		if got := nearCursor(test.cursor, work, test.width, test.height); got != test.want {
			t.Errorf("cursor %v, window %dx%d: got %v, want %v", test.cursor, test.width, test.height, got, test.want)
		}
	}
}
