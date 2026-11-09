package main

import (
	"context"
	"encoding/json"
	"math"
	"strconv"
	"time"

	"github.com/egoist/mygo"
)

type windowPreferences struct {
	IdleDestroy string `json:"app.idle_destroy_enabled"`
	IdleSeconds string `json:"app.idle_destroy_seconds"`
	EdgeDocking string `json:"app.edge_docking"`
	GpuDisabled string `json:"app.disable_webview_gpu"`
	FollowMouse string `json:"app.follow_mouse"`
	Width       string `json:"app.window_width"`
	Height      string `json:"app.window_height"`
}

func (l *Library) windowPreferences(ctx context.Context) windowPreferences {
	ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	values, err := l.Preferences(ctx)
	var settings windowPreferences
	if err != nil || json.Unmarshal(values, &settings) != nil {
		return windowPreferences{FollowMouse: "false"}
	}
	return settings
}

func (p windowPreferences) savedSize() mygo.Size {
	width, _ := strconv.Atoi(p.Width)
	height, _ := strconv.Atoi(p.Height)
	if width < 200 || height < 200 || width > 32768 || height > 32768 {
		return mygo.Size{}
	}
	return mygo.Size{Width: width, Height: height}
}

func scaledSize(width, height int, scale float64) mygo.Size {
	return mygo.Size{Width: int(math.Round(float64(width) * scale)), Height: int(math.Round(float64(height) * scale))}
}

func visibleMainPosition(bounds, area mygo.Rectangle) mygo.Point {
	axis := func(value, origin, space, size int) int {
		margin := min(16, max(0, (space-size)/2))
		return max(origin+margin, min(value, origin+max(0, space-size)-margin))
	}
	return mygo.Point{X: axis(bounds.X, area.X, area.Width, bounds.Width), Y: axis(bounds.Y, area.Y, area.Height, bounds.Height)}
}

func nearCursor(cursor mygo.Point, work mygo.Rectangle, width, height int) mygo.Point {
	const gap = 12
	const margin = 16
	x := cursor.X - width/2
	y := cursor.Y + gap
	if y+height > work.Y+work.Height {
		y = cursor.Y - gap - height
		if y < work.Y {
			y = work.Y + work.Height - height - margin
		}
	}
	limit := func(v, origin, available, size int) int {
		if size >= available {
			return origin
		}
		inset := min(margin, (available-size)/2)
		return max(origin+inset, min(v, origin+available-size-inset))
	}
	return mygo.Point{X: limit(x, work.X, work.Width, width), Y: limit(y, work.Y, work.Height, height)}
}
