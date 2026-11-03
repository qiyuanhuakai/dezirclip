//go:build windows

package client

import (
    "context"
    "net"
    "os"
    "strings"
    "unsafe"

    "github.com/Microsoft/go-winio"
    "golang.org/x/sys/windows"
)

func tokenIdentity(token windows.Token) (string, uint32, error) {
    user, err := token.GetTokenUser()
    if err != nil { return "", 0, err }
    var session, length uint32
    err = windows.GetTokenInformation(token, windows.TokenSessionId, (*byte)(unsafe.Pointer(&session)), 4, &length)
    return user.User.Sid.String(), session, err
}

func dialLocal(ctx context.Context, address string) (net.Conn, error) {
    const prefix = `\\.\pipe\dezirclip-`
    if !strings.HasPrefix(address, prefix) || strings.ContainsAny(strings.TrimPrefix(address, prefix), `\/`) { return nil, ErrProtocol }
    // Identification permits the Host to inspect this client's identity without
    // permitting it to act as this client. Anonymous pipe defaults cannot do so.
    conn, err := winio.DialPipeAccessImpLevel(ctx, address, windows.GENERIC_READ | windows.GENERIC_WRITE, winio.PipeImpLevelIdentification)
    if err != nil { return nil, err }
    handle, ok := conn.(interface { Fd() uintptr })
    if !ok { conn.Close(); return nil, ErrProtocol }
    var pid uint32
    if err = windows.GetNamedPipeServerProcessId(windows.Handle(handle.Fd()), &pid); err != nil { conn.Close(); return nil, err }
    process, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
    if err != nil { conn.Close(); return nil, err }
    defer windows.CloseHandle(process)
    var token windows.Token
    if err = windows.OpenProcessToken(process, windows.TOKEN_QUERY, &token); err != nil { conn.Close(); return nil, err }
    defer token.Close()
    actualUser, actualSession, err := tokenIdentity(token)
    if err != nil { conn.Close(); return nil, err }
    expectedUser, expectedSession, err := tokenIdentity(windows.GetCurrentProcessToken())
    if err != nil || actualUser != expectedUser || actualSession != expectedSession { conn.Close(); return nil, ErrProtocol }
    return conn, nil
}

// Only the foreground application can transfer its foreground right to the
// service. Resolve the PID from the authenticated, still-open pipe each time.
func grantFocusRestore(conn net.Conn) error {
    user32 := windows.NewLazySystemDLL("user32.dll")
    foreground, _, _ := user32.NewProc("GetForegroundWindow").Call()
    if foreground == 0 { return nil }
    var owner uint32
    if thread, _, _ := user32.NewProc("GetWindowThreadProcessId").Call(foreground, uintptr(unsafe.Pointer(&owner))); thread == 0 { return ErrProtocol }
    if owner != uint32(os.Getpid()) { return nil }
    handle, ok := conn.(interface{ Fd() uintptr })
    if !ok { return ErrProtocol }
    var pid uint32
    if err := windows.GetNamedPipeServerProcessId(windows.Handle(handle.Fd()), &pid); err != nil { return err }
    if pid == 0 || pid == ^uint32(0) { return ErrProtocol }
    if granted, _, err := user32.NewProc("AllowSetForegroundWindow").Call(uintptr(pid)); granted == 0 { return err }
    return nil
}
