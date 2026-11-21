package main

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestMyGoAppImageReplacementRestoresThePreviousVersionOnLaunchFailure(t *testing.T) {
	launchError := errors.New("loader failed")
	for _, test := range []struct {
		name      string
		launchErr error
		want      string
	}{
		{"started", nil, "new app"},
		{"failed to start", launchError, "previous app"},
	} {
		t.Run(test.name, func(t *testing.T) {
			directory := t.TempDir()
			target := filepath.Join(directory, "DezirClip.AppImage")
			staging, err := os.MkdirTemp(directory, ".dezirclip-mygo-update-")
			if err != nil {
				t.Fatal(err)
			}
			candidate := filepath.Join(staging, "candidate.AppImage")
			for path, body := range map[string]string{target: "previous app", candidate: "new app"} {
				if err := os.WriteFile(path, []byte(body), 0o755); err != nil {
					t.Fatal(err)
				}
			}
			err = replaceMyGoAppImage(candidate, target, 0o755, func() error {
				image, err := os.ReadFile(target)
				if err != nil || string(image) != "new app" {
					t.Fatalf("launch read %q: %v", image, err)
				}
				return test.launchErr
			})
			if !errors.Is(err, test.launchErr) {
				t.Fatalf("replace returned %v, want %v", err, test.launchErr)
			}
			image, err := os.ReadFile(target)
			if err != nil || string(image) != test.want {
				t.Fatalf("installed image is %q, want %q: %v", image, test.want, err)
			}
			if _, err := os.Stat(filepath.Join(staging, "previous.AppImage")); !os.IsNotExist(err) {
				t.Fatalf("staging still contains the previous image: %v", err)
			}
		})
	}
}

func TestMyGoAppImagePreflightRejectsAnUnlaunchableImage(t *testing.T) {
	candidate := filepath.Join(t.TempDir(), "candidate.AppImage")
	if err := os.WriteFile(candidate, []byte("\x7fELF\x00\x00\x00\x00AI\x02"), 0o700); err != nil {
		t.Fatal(err)
	}
	if !mygoAppImage(candidate) {
		t.Fatal("expected the fixture to pass the AppImage header check")
	}
	if err := preflightMyGoUpdate(context.Background(), &mygoInstallRequest{Path: candidate, Version: "1.0.0"}); !errors.Is(err, errUpdateInstall) {
		t.Fatalf("unlaunchable AppImage returned %v", err)
	}
}

func TestMyGoAppImageReplacementWaitsForTheNewWindow(t *testing.T) {
	for _, test := range []struct {
		name, script string
		timeout      time.Duration
		wantErr      error
		wantOld      bool
		keepBackup   bool
	}{
		{"visible window", "printf '%s' '" + mygoUpdateWindowMarker + "' >&3\n", 5 * time.Second, nil, false, false},
		{"exit before showing a window", "exit 17\n", 5 * time.Second, errUpdateInstall, true, false},
		{"window still pending", "sleep 0.25\n", 30 * time.Millisecond, errUpdateWindowUnconfirmed, false, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			root := t.TempDir()
			target := filepath.Join(root, "DezirClip.AppImage")
			if err := os.WriteFile(target, []byte("previous app"), 0o755); err != nil {
				t.Fatal(err)
			}
			staging, err := os.MkdirTemp(root, ".dezirclip-mygo-update-")
			if err != nil {
				t.Fatal(err)
			}
			candidate := filepath.Join(staging, "candidate.AppImage")
			script := "#!/bin/sh\n" + test.script
			if err = os.WriteFile(candidate, []byte(script), 0o755); err != nil {
				t.Fatal(err)
			}
			err = replaceMyGoAppImage(candidate, target, 0o755, func() error {
				return launchMyGoAppImage(exec.Command(target, "--show"), true, test.timeout)
			})
			if !errors.Is(err, test.wantErr) {
				t.Fatalf("replace returned %v, want %v", err, test.wantErr)
			}
			want := script
			if test.wantOld {
				want = "previous app"
			}
			if installed, err := os.ReadFile(target); err != nil || string(installed) != want {
				t.Fatalf("installed image is %q: %v", installed, err)
			}
			backup, err := os.ReadFile(filepath.Join(staging, "previous.AppImage"))
			if test.keepBackup {
				if err != nil || string(backup) != "previous app" {
					t.Fatalf("pending launch lost its previous image: %q, %v", backup, err)
				}
			} else if !os.IsNotExist(err) {
				t.Fatalf("completed handoff still has a staged backup: %v", err)
			}
		})
	}
}
