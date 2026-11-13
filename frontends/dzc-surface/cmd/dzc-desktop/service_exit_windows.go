package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
)

func observeServiceExit(owner *client.Client) func() {
	ctx, cancel := context.WithCancel(context.Background())
	database, err := owner.DesktopDatabase()
	if err != nil {
		return cancel
	}
	err = owner.ObserveDesktopServiceExit(ctx, func(pid, code uint32) {
		// The observer survives native crashes/abort, which cannot execute Rust's
		// panic hook. Only a timestamp, PID and kernel exit status are persisted.
		path := filepath.Join(filepath.Dir(database), "desktop-service-exit.log")
		if info, err := os.Stat(path); err == nil && info.Size() > 64*1024 {
			_ = os.Rename(path, filepath.Join(filepath.Dir(database), "desktop-service-exit.previous.log"))
		}
		file, err := os.OpenFile(path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
		if err != nil {
			log.Printf("Service exit diagnostic unavailable: %v", err)
			return
		}
		defer file.Close()
		_, _ = fmt.Fprintf(file, "%d pid=%d exit_code=0x%08x\n", time.Now().UnixMilli(), pid, code)
	})
	if err != nil {
		log.Printf("Service exit observation unavailable: %v", err)
	}
	return cancel
}
