//go:build linux

package client

import (
    "context"
    "net"
    "os"
    "syscall"

    "golang.org/x/sys/unix"
)

func desktopCLIProcessAttrs() *syscall.SysProcAttr { return nil }

func dialLocal(ctx context.Context, address string, expectedPID uint32) (net.Conn, error) {
    dialer := net.Dialer{}
    conn, err := dialer.DialContext(ctx, "unix", address)
    if err != nil { return nil, err }
    raw, err := conn.(*net.UnixConn).SyscallConn()
    if err != nil { conn.Close(); return nil, err }
    var peer *unix.Ucred
    var nativeError error
    if err = raw.Control(func(fd uintptr) { peer, nativeError = unix.GetsockoptUcred(int(fd), unix.SOL_SOCKET, unix.SO_PEERCRED) }); err != nil { conn.Close(); return nil, err }
    if nativeError != nil || peer == nil || peer.Uid != uint32(os.Geteuid()) || (expectedPID != 0 && uint32(peer.Pid) != expectedPID) { conn.Close(); return nil, ErrProtocol }
    return conn, nil
}

func grantFocusRestore(_ net.Conn) error { return nil }

func openControlFile(path string) (*os.File, error) {
    fd, err := unix.Open(path, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_NONBLOCK|unix.O_CLOEXEC, 0)
    if err != nil { return nil, err }
    var stat unix.Stat_t
    if err := unix.Fstat(fd, &stat); err != nil || stat.Mode&unix.S_IFMT != unix.S_IFREG ||
        stat.Uid != uint32(os.Geteuid()) || stat.Mode&0077 != 0 {
        unix.Close(fd)
        return nil, ErrProtocol
    }
    return os.NewFile(uintptr(fd), path), nil
}
