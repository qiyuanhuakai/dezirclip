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

const updateWindowPipeEnv = "DZC_MYGO_UPDATE_WINDOW_PIPE"
const updateWindowTimeout = 60 * time.Second

var errUpdateWindowUnconfirmed = errors.New("the replacement AppImage has not confirmed its window opened")

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

func preflightMyGoUpdate(ctx context.Context, request *mygoInstallRequest) error {
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
		Component         string `json:"component"`
		Version           string `json:"component_version"`
		UpdateWindowReady bool   `json:"update_window_ready"`
	}
	if err := json.Unmarshal(output, &info); err != nil || info.Component != "mygo-desktop" || info.Version != request.Version {
		return fmt.Errorf("%w: candidate reported an unexpected desktop build", errUpdateInstall)
	}
	request.waitForWindow = info.UpdateWindowReady
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
		return launchMyGoAppImage(child, request.waitForWindow, updateWindowTimeout)
	})
}

func takeMyGoUpdateWindowPipe() *os.File {
	if os.Getenv(updateWindowPipeEnv) != "1" {
		return nil
	}
	_ = os.Unsetenv(updateWindowPipeEnv)
	// Only the new resident can acknowledge readiness. Its service and UI children must not hold the pipe open.
	const fd = 3
	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil || stat.Mode&unix.S_IFMT != unix.S_IFIFO {
		return nil
	}
	unix.CloseOnExec(fd)
	return os.NewFile(fd, "appimage-window-ready")
}

func launchMyGoAppImage(child *exec.Cmd, waitForWindow bool, timeout time.Duration) error {
	if !waitForWindow {
		if err := child.Start(); err != nil {
			return err
		}
		_ = child.Process.Release()
		return nil
	}
	reader, writer, err := os.Pipe()
	if err != nil {
		return err
	}
	defer reader.Close()
	defer writer.Close()
	if err = reader.SetReadDeadline(time.Now().Add(timeout)); err != nil {
		return err
	}
	child.ExtraFiles = []*os.File{writer}
	child.Env = append(child.Environ(), updateWindowPipeEnv+"=1")
	if err = child.Start(); err != nil {
		return err
	}
	_ = writer.Close()
	go func() { _ = child.Wait() }()
	marker := make([]byte, len(mygoUpdateWindowMarker))
	if _, err = io.ReadFull(reader, marker); err == nil && string(marker) == mygoUpdateWindowMarker {
		return nil
	}
	if errors.Is(err, io.EOF) {
		return fmt.Errorf("%w: the replacement AppImage exited before opening a window", errUpdateInstall)
	}
	return errors.Join(errUpdateWindowUnconfirmed, err)
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
		if errors.Is(err, errUpdateWindowUnconfirmed) {
			// A slow process may still be using the new image. Keep both launchers in place.
			return fmt.Errorf("%w (previous AppImage retained at %q)", err, backup)
		}
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
