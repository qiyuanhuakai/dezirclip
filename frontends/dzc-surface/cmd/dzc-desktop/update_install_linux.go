package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"golang.org/x/sys/unix"
)

func mygoUpdateTarget() (path, support string) {
	executable, err := os.Executable()
	if err != nil {
		return "", "updater_support_unavailable"
	}
	image, appDir := os.Getenv("APPIMAGE"), os.Getenv("APPDIR")
	if !filepath.IsAbs(image) || !filepath.IsAbs(appDir) ||
		!strings.HasPrefix(executable, filepath.Clean(appDir)+string(filepath.Separator)) {
		if filepath.Dir(executable) == "/usr/lib/DezirClip-MyGo" {
			return "", "updater_support_package"
		}
		return "", "updater_support_portable"
	}
	image, err = filepath.EvalSymlinks(image)
	if err != nil {
		return "", "updater_support_readonly"
	}
	info, err := os.Stat(image)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0o111 == 0 ||
		!mygoAppImage(image) || unix.Access(filepath.Dir(image), unix.W_OK|unix.X_OK) != nil ||
		unix.Access(image, unix.W_OK) != nil {
		return "", "updater_support_readonly"
	}
	return image, "updater_support_appimage"
}

func mygoAppImage(path string) bool {
	file, err := os.Open(path)
	if err != nil {
		return false
	}
	defer file.Close()
	var header [11]byte
	_, err = io.ReadFull(file, header[:])
	return err == nil && bytes.Equal(header[:4], []byte("\x7fELF")) && bytes.Equal(header[8:], []byte("AI\x02"))
}

func prepareMyGoUpdate(request mygoInstallRequest) (*mygoUpdateHandoff, error) {
	return &mygoUpdateHandoff{
		finish: func() error { return installMyGoUpdate(request) },
		abort:  func() {},
	}, nil
}

func preflightMyGoUpdate(ctx context.Context, request mygoInstallRequest) error {
	if !mygoAppImage(request.Path) {
		return fmt.Errorf("%w: candidate is not an AppImage", errUpdateInstall)
	}
	ctx, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, request.Path, "--build-info")
	command.Dir = filepath.Dir(request.Path)
	command.Env = mygoAppImageEnvironment(os.Environ())
	command.WaitDelay = 2 * time.Second
	output, err := command.Output()
	if err != nil {
		return fmt.Errorf("%w: read candidate build info: %v", errUpdateInstall, err)
	}
	var info struct {
		Component string `json:"component"`
		Version   string `json:"component_version"`
	}
	if err := json.Unmarshal(output, &info); err != nil || info.Component != "mygo-desktop" || info.Version != request.Version {
		return fmt.Errorf("%w: candidate reported an unexpected desktop build", errUpdateInstall)
	}
	return nil
}

func installMyGoUpdate(request mygoInstallRequest) error {
	target, _ := mygoUpdateTarget()
	directory := filepath.Dir(request.Path)
	if target == "" || filepath.Dir(directory) != filepath.Dir(target) || !mygoAppImage(request.Path) {
		return errors.New("update_installation_unsupported")
	}
	info, err := os.Stat(target)
	if err != nil {
		return err
	}
	if err = os.Chmod(request.Path, info.Mode().Perm()); err != nil {
		return err
	}
	return replaceMyGoAppImage(request.Path, target, info.Mode().Perm(), func() error {
		child := exec.Command(target, "--show")
		child.Dir = filepath.Dir(target)
		child.Env, child.Stderr = mygoAppImageEnvironment(os.Environ()), os.Stderr
		if err := child.Start(); err != nil {
			return err
		}
		_ = child.Process.Release()
		return nil
	})
}

func replaceMyGoAppImage(candidate, target string, mode os.FileMode, start func() error) error {
	directory := filepath.Dir(candidate)
	backup := filepath.Join(directory, "previous.AppImage")
	if err := keepPreviousMyGoAppImage(target, backup, mode); err != nil {
		return err
	}
	if err := os.Rename(candidate, target); err != nil {
		_ = os.Remove(backup)
		return err
	}
	if err := start(); err != nil {
		// Restore the previous launcher atomically. If restoration fails, leave
		// the backup in staging so the original binary can still be recovered.
		return errors.Join(err, os.Rename(backup, target))
	}
	_ = os.Remove(backup)
	_ = os.Remove(directory)
	return nil
}

func keepPreviousMyGoAppImage(target, backup string, mode os.FileMode) error {
	if err := os.Link(target, backup); err == nil {
		return nil
	}
	input, err := os.Open(target)
	if err != nil {
		return err
	}
	defer input.Close()
	output, err := os.OpenFile(backup, os.O_CREATE|os.O_EXCL|os.O_WRONLY, mode)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(output, input)
	if err := errors.Join(copyErr, output.Chmod(mode), output.Close()); err != nil {
		_ = os.Remove(backup)
		return err
	}
	return nil
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
