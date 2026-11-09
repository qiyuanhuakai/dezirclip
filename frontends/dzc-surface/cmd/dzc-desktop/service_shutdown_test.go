package main

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
)

func TestDesktopServiceShutdown(t *testing.T) {
	version := os.Getenv("DZC_TEST_LIBRARY_VERSION")
	if version == "" {
		t.Skip("requires sibling CLI and Service")
	}
	t.Setenv("DEZIRCLIP_DB_PATH", filepath.Join(t.TempDir(), "clipboard.db"))
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	cli := filepath.Join(filepath.Dir(executable), "dzc")
	if runtime.GOOS == "windows" {
		cli += ".exe"
	}
	start := func() *client.Client {
		if out, err := exec.Command(cli, "service", "start", "--no-capture").CombinedOutput(); err != nil {
			t.Fatalf("start: %v %s", err, out)
		}
		owner, err := client.OpenDesktop(context.Background(), version, false)
		if err != nil {
			t.Fatal(err)
		}
		owner.Close()
		return owner
	}
	t.Cleanup(func() { _ = exec.Command(cli, "service", "stop").Run() })
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	first := start()
	if err := first.StopDesktopService(ctx); err != nil {
		t.Fatal(err)
	}
	if _, err := client.OpenDesktop(ctx, version, false); err == nil {
		t.Fatal("stopped Service still accepts clients")
	}
	second := start()
	if first.Hello().State.ServiceInstanceId == second.Hello().State.ServiceInstanceId {
		t.Fatal("replacement reused instance")
	}
	if err := first.StopDesktopService(ctx); err == nil {
		t.Fatal("stale Desktop accepted replacement Service")
	}
	if _, err := (&Library{second}).Status(ctx); err != nil {
		t.Fatalf("replacement was stopped: %v", err)
	}
	if err := second.StopDesktopService(ctx); err != nil {
		t.Fatal(err)
	}
}
