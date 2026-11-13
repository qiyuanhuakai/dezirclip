package client

import (
	"context"
	"time"

	"golang.org/x/sys/windows"
)

// ObserveDesktopServiceExit retains a process handle while the authenticated
// pipe is live. It never terminates or restarts the Service, including on cancel.
func (c *Client) ObserveDesktopServiceExit(ctx context.Context, report func(uint32, uint32)) error {
	if c.desktop == nil {
		return ErrProtocol
	}
	// Desktop owners intentionally close their discovery stream. Obtain a fresh
	// authenticated stream while acquiring the kernel process handle.
	probe, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	host, err := c.DesktopConnection(probe)
	if err != nil {
		return err
	}
	defer host.Close()
	pipe, ok := host.conn.(interface{ Fd() uintptr })
	if !ok {
		return ErrProtocol
	}
	var pid uint32
	if err := windows.GetNamedPipeServerProcessId(windows.Handle(pipe.Fd()), &pid); err != nil {
		return err
	}
	if pid != c.desktop.PID {
		return ErrProtocol
	}
	process, err := windows.OpenProcess(windows.SYNCHRONIZE|windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return err
	}
	go func() {
		defer windows.CloseHandle(process)
		for ctx.Err() == nil {
			status, err := windows.WaitForSingleObject(process, 250)
			if err != nil {
				return
			}
			if status == windows.WAIT_OBJECT_0 {
				var code uint32
				if windows.GetExitCodeProcess(process, &code) == nil && ctx.Err() == nil {
					report(pid, code)
				}
				return
			}
		}
	}()
	return nil
}
