package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

type VideoLabels struct {
	Play, Pause, Progress, Mute, Unmute, Volume, Loop string
}

// PinnedVideo draws a playback viewport and controls within the preview surface.
func (k Kit) PinnedVideo(c *ui.Context, player *ui.VideoPlayer, labels VideoLabels) {
	ui.Video(c, player).FillWidth().Grow(1).MinHeight(0).Fit(ui.Contain)
	duration := player.Duration()
	progress := player.Position().Seconds()
	if duration > 0 {
		if k.videoSlider(c.Key("position"), &progress, duration.Seconds(), .1, labels.Progress).FillWidth().Changed() {
			player.Seek(time.Duration(progress * float64(time.Second)))
		}
	}
	ui.Row(c).FillWidth().Gap(6).AlignItems(ui.Center).Children(func() {
		if player.Playing() {
			if k.Button(c, ButtonOptions{Label: labels.Pause}).Clicked() {
				player.Pause()
			}
		} else if k.Button(c, ButtonOptions{Label: labels.Play}).Clicked() {
			player.Play()
		}
		mute := labels.Mute
		if player.Muted() {
			mute = labels.Unmute
		}
		if k.Button(c, ButtonOptions{Label: mute}).Clicked() {
			player.SetMuted(!player.Muted())
		}
		volume := player.Volume()
		if k.videoSlider(c.Key("volume"), &volume, 1, .05, labels.Volume).Grow(1).MinWidth(36).Changed() {
			player.SetVolume(volume)
		}
		if k.Button(c, ButtonOptions{Label: labels.Loop, Selected: player.Loop()}).Clicked() {
			player.SetLoop(!player.Loop())
		}
	})
}

func (k Kit) videoSlider(c *ui.Context, value *float64, maximum, step float64, label string) ui.Element {
	track, thumb := k.style("previewOpacityTrack"), k.style("previewOpacityThumb")
	slider := ui.SliderBase(c, value, 0, maximum).Height(18).PaddingX(thumb.Width / 2).
		Step(step).FocusRing(true).Label(label).Tooltip(label)
	return slider.Draw(func(p *ui.Painter, r ui.Rect) {
		line := ui.Rect{X: r.X, Y: r.Y + (r.H-track.Height)/2, W: r.W, H: track.Height}
		p.Fill(line, styleColor(track.Fill), track.Radius)
		p.Stroke(line, styleColor(track.Border), track.Radius, track.BorderWidth)
		knob := ui.Rect{X: r.X + max(0, r.W-thumb.Width)*float32(*value/maximum),
			Y: r.Y + (r.H-thumb.Height)/2, W: thumb.Width, H: thumb.Height}
		p.Fill(knob, styleColor(thumb.Fill), thumb.Radius)
		p.Stroke(knob, styleColor(thumb.Border), thumb.Radius, thumb.BorderWidth)
	})
}
