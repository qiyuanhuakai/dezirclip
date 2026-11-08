package main

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"
	"unsafe"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
	"golang.org/x/sys/windows"
)

type shellExecuteInfo struct {
	Size, Mask                        uint32
	Window                            uintptr
	Verb, File, Parameters, Directory *uint16
	Show                              int32
	Instance                          windows.Handle
	IDList                            uintptr
	Class                             *uint16
	ClassKey                          windows.Handle
	HotKey                            uint32
	Icon, Process                     windows.Handle
}

func elevateDesktop(instance, database string) error {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	if err := windows.CoInitializeEx(0, windows.COINIT_APARTMENTTHREADED|windows.COINIT_DISABLE_OLE1DDE); err != nil && err != syscall.Errno(1) {
		return err
	}
	defer windows.CoUninitialize()
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	file, err := windows.UTF16PtrFromString(executable)
	if err != nil {
		return err
	}
	parameters, err := windows.UTF16PtrFromString(strings.Join([]string{"--elevate-helper", windows.EscapeArg(instance), strconv.Itoa(os.Getpid()), windows.EscapeArg(database)}, " "))
	if err != nil {
		return err
	}
	verb, _ := windows.UTF16PtrFromString("runas")
	info := shellExecuteInfo{Mask: 0x40 | 0x100, Verb: verb, File: file, Parameters: parameters, Show: 0}
	info.Size = uint32(unsafe.Sizeof(info))
	ok, _, callErr := windows.NewLazySystemDLL("shell32.dll").NewProc("ShellExecuteExW").Call(uintptr(unsafe.Pointer(&info)))
	runtime.KeepAlive(info)
	if ok == 0 {
		return callErr
	}
	if info.Process == 0 {
		return errors.New("ElevatedHelperHandleUnavailable")
	}
	defer windows.CloseHandle(info.Process)
	if result, err := windows.WaitForSingleObject(info.Process, windows.INFINITE); err != nil || result != windows.WAIT_OBJECT_0 {
		return fmt.Errorf("ElevatedHelperWaitFailed: %v", err)
	}
	var code uint32
	if err := windows.GetExitCodeProcess(info.Process, &code); err != nil {
		return err
	}
	if code != 0 {
		return fmt.Errorf("ElevatedHelperFailed: %d", code)
	}
	return nil
}

func elevationParent(pid uint32, allowExited bool) (windows.Handle, error) {
	if pid == 0 || pid == uint32(os.Getpid()) {
		return 0, errors.New("InvalidElevationParent")
	}
	process, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.SYNCHRONIZE, false, pid)
	if err != nil {
		if allowExited && errors.Is(err, windows.ERROR_INVALID_PARAMETER) {
			return 0, nil
		}
		return 0, err
	}
	valid := false
	defer func() {
		if !valid {
			windows.CloseHandle(process)
		}
	}()
	buffer := make([]uint16, 32768)
	length := uint32(len(buffer))
	if err := windows.QueryFullProcessImageName(process, 0, &buffer[0], &length); err != nil {
		return 0, err
	}
	executable, err := os.Executable()
	if err != nil {
		return 0, err
	}
	if !strings.EqualFold(filepath.Clean(windows.UTF16ToString(buffer[:length])), filepath.Clean(executable)) {
		return 0, errors.New("ElevationParentExecutableMismatch")
	}
	var token windows.Token
	if err := windows.OpenProcessToken(process, windows.TOKEN_QUERY, &token); err != nil {
		return 0, err
	}
	defer token.Close()
	parent, err := token.GetTokenUser()
	if err != nil {
		return 0, err
	}
	current, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		return 0, err
	}
	if !parent.User.Sid.Equals(current.User.Sid) {
		return 0, errors.New("ElevationUserMismatch")
	}
	valid = true
	return process, nil
}

func prepareElevation() (bool, error) {
	if len(os.Args) < 2 || (os.Args[1] != "--elevate-helper" && os.Args[1] != "--after-elevation") {
		return false, nil
	}
	if len(os.Args) != 5 || !windows.GetCurrentProcessToken().IsElevated() || !filepath.IsAbs(os.Args[4]) {
		return true, errors.New("InvalidElevatedDesktopArguments")
	}
	helper := os.Args[1] == "--elevate-helper"
	instance, database := os.Args[2], os.Args[4]
	pid, err := strconv.ParseUint(os.Args[3], 10, 32)
	if err != nil {
		return true, err
	}
	parent, err := elevationParent(uint32(pid), !helper)
	if err != nil {
		return true, err
	}
	if parent != 0 {
		defer windows.CloseHandle(parent)
	}
	if err := os.Setenv("DEZIRCLIP_DB_PATH", database); err != nil {
		return true, err
	}
	if helper {
		ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
		defer cancel()
		status, err := client.RestartElevatedService(ctx, instance)
		if err != nil {
			return true, err
		}
		executable, err := os.Executable()
		if err != nil {
			return true, err
		}
		child := exec.Command(executable, "--after-elevation", status.State.ServiceInstanceId, strconv.FormatUint(pid, 10), database)
		stdout, err := child.StdoutPipe()
		if err != nil {
			return true, err
		}
		defer stdout.Close()
		if err := child.Start(); err != nil {
			return true, err
		}
		ready := make(chan error, 1)
		go func() {
			line, err := bufio.NewReaderSize(io.LimitReader(stdout, 64), 64).ReadString('\n')
			if err == nil && line != "ready\n" {
				err = errors.New("ElevatedDesktopNotReady")
			}
			ready <- err
		}()
		select {
		case err = <-ready:
		case <-ctx.Done():
			err = ctx.Err()
		}
		if err != nil {
			_ = child.Process.Kill()
			_ = child.Wait()
			return true, err
		}
		return true, child.Process.Release()
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	owner, err := client.OpenDesktop(ctx, buildinfo.Version, false)
	if err != nil {
		return true, err
	}
	owner.Close()
	if owner.Hello().State.ServiceInstanceId != instance {
		return true, client.ErrProtocol
	}
	admin, err := (&SystemSettings{owner: owner}).IsAdmin(ctx)
	if err != nil {
		return true, err
	}
	if !admin {
		return true, errors.New("HostElevationUnconfirmed")
	}
	if _, err := fmt.Fprintln(os.Stdout, "ready"); err != nil {
		return true, err
	}
	if parent != 0 {
		if result, err := windows.WaitForSingleObject(parent, 30000); err != nil || result != windows.WAIT_OBJECT_0 {
			return true, fmt.Errorf("PreviousDesktopExitUnconfirmed: %v", err)
		}
	}
	elevatedInstance = instance
	os.Args = []string{os.Args[0], "--show"}
	return false, nil
}
