package main

import (
	"context"
	"encoding/json"
	"time"

	"github.com/egoist/mygo"
)

func (l *Library) followMouse(ctx context.Context) bool {
	ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	values, err := l.Preferences(ctx)
	var setting struct {
		FollowMouse string `json:"app.follow_mouse"`
	}
	return err == nil && json.Unmarshal(values, &setting) == nil && setting.FollowMouse != "false"
}

func nearCursor(cursor mygo.Point, work mygo.Rectangle, width, height int) mygo.Point {
	const gap = 12
	const margin = 5
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
