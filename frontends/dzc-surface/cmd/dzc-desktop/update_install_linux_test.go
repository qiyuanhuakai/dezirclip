package main

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
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
	if err := preflightMyGoUpdate(context.Background(), mygoInstallRequest{Path: candidate, Version: "1.0.0"}); !errors.Is(err, errUpdateInstall) {
		t.Fatalf("unlaunchable AppImage returned %v", err)
	}
}
