//go:build linux

package client

import (
    "context"
    "net"
    "os"

    "golang.org/x/sys/unix"
)

func dialLocal(ctx context.Context, address string) (net.Conn, error) {
    dialer := net.Dialer{}
    conn, err := dialer.DialContext(ctx, "unix", address)
    if err != nil { return nil, err }
    raw, err := conn.(*net.UnixConn).SyscallConn()
    if err != nil { conn.Close(); return nil, err }
    var peer *unix.Ucred
    var nativeError error
    if err = raw.Control(func(fd uintptr) { peer, nativeError = unix.GetsockoptUcred(int(fd), unix.SOL_SOCKET, unix.SO_PEERCRED) }); err != nil { conn.Close(); return nil, err }
    if nativeError != nil || peer == nil || peer.Uid != uint32(os.Geteuid()) { conn.Close(); return nil, ErrProtocol }
    return conn, nil
}
