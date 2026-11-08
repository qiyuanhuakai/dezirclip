package main

import (
	"context"
	"log"
	"time"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Window events run on the UI thread. Coalesce drag events off that thread
// and flush the last observed size before a replacement WebView can start.
func (l *Library) saveWindowSizes(updates <-chan mygo.Size, done chan<- struct{}) {
	defer close(done)
	var pending mygo.Size
	var timer *time.Timer
	var tick <-chan time.Time
	save := func() {
		if pending.Width == 0 {
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		status, err := l.Status(ctx)
		if err == nil {
			err = l.edit(ctx, protocol.MethodDesktopWindowResize, protocol.WindowSizeParams{
				ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: status.State.PrivacyEpoch,
				Width: uint32(pending.Width), Height: uint32(pending.Height),
			})
		}
		if err != nil {
			log.Printf("Saving main window size: %v", err)
			return
		}
		pending = mygo.Size{}
	}
	for {
		select {
		case size, open := <-updates:
			if !open {
				if timer != nil {
					timer.Stop()
				}
				save()
				return
			}
			pending = size
			if timer == nil {
				timer = time.NewTimer(250 * time.Millisecond)
			} else {
				timer.Reset(250 * time.Millisecond)
			}
			tick = timer.C
		case <-tick:
			tick = nil
			save()
		}
	}
}
