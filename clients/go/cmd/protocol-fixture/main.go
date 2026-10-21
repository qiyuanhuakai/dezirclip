// protocol-fixture checks the real native transport against a synthetic Rust
// server. It accesses no user database, GUI or clipboard.
package main

import (
    "context"
    "fmt"
    "os"
    "time"

    "github.com/qiyuanhuakai/dezirclip/clients/go/client"
    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func run() error {
    if len(os.Args) != 2 { return fmt.Errorf("fixture endpoint required") }
    ctx, cancel := context.WithTimeout(context.Background(), 10 * time.Second)
    defer cancel()
    connection, err := client.Dial(ctx, os.Args[1], protocol.HelloParams{
        ProtocolVersion: protocol.Version, ClientVersion: "fixture", ClientRole: protocol.ClientRoleSurface,
        Capabilities: []string{"history_read"}, RequiredCapabilities: []string{},
    })
    if err != nil { return err }
    defer connection.Close()
    hello := connection.Hello()
    if hello.State.HistoryRevision != 3 || hello.State.PrivacyEpoch != 2 || len(hello.Permissions) != 1 || hello.Permissions[0] != protocol.PermissionHistoryRead { return client.ErrProtocol }
    if err = connection.Call(ctx, protocol.MethodHealth, protocol.EmptyParams{}, "EmptyParams", &protocol.EmptyParams{}); err != nil { return err }
    var history protocol.HistoryResult
    if err = connection.Call(ctx, protocol.MethodSurfaceHistoryList, protocol.HistoryPageParams{Limit:20,Offset:0}, "HistoryResult", &history); err != nil { return err }
    if len(history.Entries) != 0 { return client.ErrProtocol }
    fmt.Println("native Rust/Go protocol fixture passed")
    return nil
}

func main() { if err := run(); err != nil { fmt.Fprintln(os.Stderr, err); os.Exit(1) } }
