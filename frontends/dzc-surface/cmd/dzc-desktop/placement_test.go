package main

import (
	"testing"

	"github.com/egoist/mygo"
)

func TestDockRevealRemainsEligibleForRetraction(t *testing.T) {
	area := mygo.Rectangle{X: -1600, Y: -200, Width: 1600, Height: 900}
	for _, bounds := range []mygo.Rectangle{
		{X: -1800, Y: 40, Width: 600, Height: 500},
		{X: -300, Y: 40, Width: 600, Height: 500},
		{X: -1000, Y: -400, Width: 600, Height: 500},
	} {
		point := visibleMainPosition(bounds, area, true)
		bounds.X, bounds.Y = point.X, point.Y
		if bounds.X < area.X || bounds.Y < area.Y || bounds.X+bounds.Width > area.X+area.Width || bounds.Y+bounds.Height > area.Y+area.Height {
			t.Fatalf("dock reveal remains clipped: %v", bounds)
		}
		if (dockSample{window: bounds, monitor: area}).strip() == (mygo.Rectangle{}) {
			t.Fatalf("dock reveal lost its edge: %v", bounds)
		}
		ordinary := visibleMainPosition(bounds, area, false)
		bounds.X, bounds.Y = ordinary.X, ordinary.Y
		if (dockSample{window: bounds, monitor: area}).strip() != (mygo.Rectangle{}) {
			t.Fatalf("ordinary activation immediately docks: %v", bounds)
		}
	}
}

func TestMainWindowPlacement(t *testing.T) {
	work := mygo.Rectangle{X: -1600, Y: -200, Width: 1600, Height: 860}
	for _, test := range []struct {
		cursor        mygo.Point
		width, height int
		want          mygo.Point
	}{
		{mygo.Point{X: -800, Y: -100}, 660, 500, mygo.Point{X: -1130, Y: -88}},
		{mygo.Point{X: -5, Y: 640}, 660, 500, mygo.Point{X: -676, Y: 128}},
		{mygo.Point{X: -1599, Y: 200}, 660, 620, mygo.Point{X: -1584, Y: 24}},
		{mygo.Point{X: -800, Y: 200}, 2000, 1000, mygo.Point{X: -1600, Y: -200}},
	} {
		if got := nearCursor(test.cursor, work, test.width, test.height); got != test.want {
			t.Errorf("cursor %v, window %dx%d: got %v, want %v", test.cursor, test.width, test.height, got, test.want)
		}
	}
}
