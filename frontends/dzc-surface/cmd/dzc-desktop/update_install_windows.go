package main

import (
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

func installMyGoUpdate(request mygoInstallRequest) error {
	if target, _ := mygoUpdateTarget(); target == "" {
		return errors.New("update_installation_unsupported")
	}
	child := exec.Command(request.Path, "/S", "/UPDATEPARENT="+strconv.Itoa(os.Getpid()))
	child.SysProcAttr = &syscall.SysProcAttr{CreationFlags: windows.CREATE_NEW_PROCESS_GROUP | windows.DETACHED_PROCESS}
	if err := child.Start(); err != nil {
		return err
	}
	return child.Process.Release()
}
