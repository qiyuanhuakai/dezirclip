package main

import (
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

const profileRelaunchArg = "--after-profile-move"

// Held until the resident exits so its replacement cannot start on a still-owned desktop.
var profileRelaunchPipe io.WriteCloser

type profileReplacement struct {
	pipe    io.WriteCloser
	process *os.Process
}

func startProfileReplacement(executable string) (*profileReplacement, error) {
	child := exec.Command(executable, profileRelaunchArg)
	if image, mount := os.Getenv("APPIMAGE"), os.Getenv("APPDIR"); runtime.GOOS == "linux" &&
		filepath.IsAbs(image) && filepath.IsAbs(mount) &&
		strings.HasPrefix(executable, filepath.Clean(mount)+string(filepath.Separator)) {
		child = exec.Command(image, profileRelaunchArg)
		child.Env = mygoAppImageEnvironment(os.Environ())
	}
	child.Stderr = os.Stderr
	pipe, err := child.StdinPipe()
	if err != nil {
		return nil, err
	}
	if err := child.Start(); err != nil {
		_ = pipe.Close()
		return nil, err
	}
	return &profileReplacement{pipe: pipe, process: child.Process}, nil
}

func (r *profileReplacement) commit() error {
	if _, err := io.WriteString(r.pipe, "R"); err != nil {
		return err
	}
	profileRelaunchPipe, r.pipe = r.pipe, nil
	return nil
}

func (r *profileReplacement) close() {
	if r.pipe != nil {
		_ = r.pipe.Close()
	}
	_ = r.process.Release()
}

func acceptProfileRelaunch(input io.Reader) bool {
	var marker [1]byte
	if _, err := io.ReadFull(input, marker[:]); err != nil || marker[0] != 'R' {
		return false
	}
	_, err := io.ReadFull(input, marker[:])
	return errors.Is(err, io.EOF)
}

func mygoAppImageEnvironment(inherited []string) []string {
	environment := make([]string, 0, len(inherited))
	mount := os.Getenv("APPDIR")
	for _, entry := range inherited {
		key, value, _ := strings.Cut(entry, "=")
		switch key {
		case "APPIMAGE", "APPDIR", "ARGV0", "OWD":
			continue
		case "PATH", "LD_LIBRARY_PATH", "XDG_DATA_DIRS":
			var paths []string
			for _, path := range filepath.SplitList(value) {
				if mount == "" || (path != mount && !strings.HasPrefix(path, mount+"/")) {
					paths = append(paths, path)
				}
			}
			entry = key + "=" + strings.Join(paths, string(os.PathListSeparator))
		}
		environment = append(environment, entry)
	}
	return environment
}

// Only the child process handle created by this resident may be terminated.
// The Service uses its authenticated drain protocol instead of a process kill.
func waitDesktopExit(done <-chan struct{}, kill func() error, grace time.Duration) error {
	select {
	case <-done:
		return nil
	case <-time.After(grace):
	}
	if err := kill(); err != nil {
		select {
		case <-done:
			return nil
		default:
			return fmt.Errorf("main window shutdown: %w", err)
		}
	}
	select {
	case <-done:
		return nil
	case <-time.After(grace):
		return fmt.Errorf("main window exit unconfirmed")
	}
}
