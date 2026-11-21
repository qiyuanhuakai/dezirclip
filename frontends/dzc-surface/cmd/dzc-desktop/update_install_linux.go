package main

import (
	"bytes"
	"errors"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

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
	if err = os.Rename(request.Path, target); err != nil {
		return err
	}
	_ = os.Remove(directory)
	environment := make([]string, 0, len(os.Environ()))
	for _, entry := range os.Environ() {
		key, value, _ := strings.Cut(entry, "=")
		switch key {
		case "APPIMAGE", "APPDIR", "ARGV0", "OWD":
			continue
		case "PATH", "LD_LIBRARY_PATH", "XDG_DATA_DIRS":
			var paths []string
			mount := os.Getenv("APPDIR")
			for _, path := range filepath.SplitList(value) {
				if path != mount && !strings.HasPrefix(path, mount+"/") {
					paths = append(paths, path)
				}
			}
			entry = key + "=" + strings.Join(paths, string(os.PathListSeparator))
		}
		environment = append(environment, entry)
	}
	child := exec.Command(target, "--show")
	child.Env, child.Stderr = environment, os.Stderr
	if err = child.Start(); err != nil {
		return err
	}
	return child.Process.Release()
}
