package main

import (
	"errors"
	"os"

	"golang.org/x/sys/windows"
)

// The resident created the pipe before starting this foreground WebView.
func grantResidentFocus() error {
	var pid uint32
	if err := windows.GetNamedPipeServerProcessId(windows.Handle(os.Stdout.Fd()), &pid); err != nil {
		return err
	}
	if pid == 0 || pid == ^uint32(0) || int(pid) != os.Getppid() {
		return errors.New("ResidentUnavailable")
	}
	ok, _, err := windows.NewLazySystemDLL("user32.dll").NewProc("AllowSetForegroundWindow").Call(uintptr(pid))
	if ok == 0 {
		return err
	}
	return nil
}
