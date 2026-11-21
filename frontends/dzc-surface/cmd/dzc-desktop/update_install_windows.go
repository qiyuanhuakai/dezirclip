package main

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"

	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

// NSIS has no build-info mode. Its signed payload is checked by the installer.
func preflightMyGoUpdate(context.Context, mygoInstallRequest) error { return nil }

func mygoUpdateTarget() (path, support string) {
	executable, err := os.Executable()
	if err != nil {
		return "", "updater_support_unavailable"
	}
	key, err := registry.OpenKey(registry.CURRENT_USER, `Software\Microsoft\Windows\CurrentVersion\Uninstall\DezirClip-MyGo`, registry.QUERY_VALUE|registry.WOW64_64KEY)
	if err != nil {
		return "", "updater_support_portable"
	}
	defer key.Close()
	dir, _, err := key.GetStringValue("InstallLocation")
	if err != nil || !filepath.IsAbs(dir) || !strings.EqualFold(filepath.Dir(executable), filepath.Clean(dir)) {
		return "", "updater_support_portable"
	}
	if _, err := os.Stat(filepath.Join(dir, "mygo-installation.ini")); err != nil {
		return "", "updater_support_portable"
	}
	return dir, "updater_support_nsis"
}

func prepareMyGoUpdate(request mygoInstallRequest) (*mygoUpdateHandoff, error) {
	if target, _ := mygoUpdateTarget(); target == "" {
		return nil, errors.New("update_installation_unsupported")
	}
	// NSIS waits for this resident to exit before opening the installed files.
	// Start it while the main window can still display a launch error.
	child := exec.Command(request.Path, "/S", "/UPDATEPARENT="+strconv.Itoa(os.Getpid()))
	child.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NEW_PROCESS_GROUP | windows.DETACHED_PROCESS}
	if err := child.Start(); err != nil {
		return nil, err
	}
	return &mygoUpdateHandoff{
		finish: child.Process.Release,
		abort: func() {
			_ = child.Process.Kill()
			_ = child.Wait()
		},
		reportFailure: func(message string) {
			text := windows.StringToUTF16Ptr(message)
			title := windows.StringToUTF16Ptr("DezirClip MyGo")
			_, _ = windows.MessageBox(0, text, title, windows.MB_ICONERROR|windows.MB_SETFOREGROUND)
		},
	}, nil
}
