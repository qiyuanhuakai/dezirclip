package main

import (
	"context"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/video"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type pinnedVideo struct {
	request *videoOpenRequest
	player  *ui.VideoPlayer
	failed  bool
}

type videoOpenRequest struct{ cancel context.CancelFunc }

func (v *pinnedVideo) close() {
	if v.request != nil {
		v.request.cancel()
	}
	if v.player != nil {
		v.player.Close()
	}
	*v = pinnedVideo{}
}

func (b *previewBody) hasLocalVideo() bool {
	if len(b.attachments) != 1 {
		return false
	}
	switch strings.ToLower(filepath.Ext(b.attachments[0].Name)) {
	case ".mp4", ".m4v", ".mov", ".avi", ".mkv", ".webm", ".wmv":
		return true
	}
	return false
}

func (p *pinnedPreview) playVideo() {
	params := p.ticket.Params
	params.ExpectedEpoch = p.opacity.epoch
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	request := &videoOpenRequest{cancel: cancel}
	p.video.request = request
	go func() {
		var source protocol.LocalVideoSource
		err := p.owner.DesktopCall(ctx, protocol.MethodDesktopHistoryLocalVideo, params, "LocalVideoSource", &source)
		p.window.Update(func() {
			defer cancel()
			if p.video.request != request {
				return
			}
			p.video.request = nil
			if err == nil && ctx.Err() == nil && source.State.ServiceInstanceId == params.ServiceInstanceId &&
				source.State.PrivacyEpoch == params.ExpectedEpoch {
				p.video.player, err = video.Open(source.Path)
				if err == nil {
					p.video.player.Play()
					return
				}
			}
			p.video.failed = true
		})
	}()
}

func (p *pinnedPreview) drawVideo(c *ui.Context, a previewAppearance, kit uikit.Kit) bool {
	if p.ticket.Params.Reveal || !p.body.hasLocalVideo() {
		return false
	}
	labels := uikit.VideoLabels{
		Play: "播放视频", Pause: "暂停", Progress: "播放进度", Mute: "静音", Unmute: "取消静音",
		Loop: "循环", Volume: "音量",
	}
	failure := "无法播放此视频。请检查文件是否仍在原路径，以及系统是否支持该格式。"
	if a.Language == "en" {
		labels = uikit.VideoLabels{Play: "Play video", Pause: "Pause", Progress: "Playback position", Mute: "Mute",
			Unmute: "Unmute", Loop: "Loop", Volume: "Volume"}
		failure = "Unable to play this video. Check that the file still exists and the system supports its format."
	}
	if player := p.video.player; player != nil {
		if player.Err() == nil {
			kit.PinnedVideo(c, player, labels)
			return true
		}
		p.video.close()
		p.video.failed = true
	}
	if p.video.failed {
		ui.Text(c, failure).Wrap().TextColor(c.Theme().TextMuted)
	}
	if kit.Button(c, uikit.ButtonOptions{Label: labels.Play, Disabled: p.video.request != nil}).Clicked() {
		p.playVideo()
	}
	return false
}
