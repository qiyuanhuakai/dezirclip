package client

import (
    "bytes"
    "context"
    "encoding/json"
    "net"
    "testing"
    "time"

    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestBoundedFramesAndShortReads(t *testing.T) {
    if _, err := readFrame(bytes.NewReader([]byte{255,255,255,255})); err == nil { t.Fatal("oversize frame accepted") }
    if _, err := readFrame(bytes.NewReader([]byte{0,0,0,2,'{'})); err == nil { t.Fatal("truncated frame accepted") }
    var buffer bytes.Buffer
    if err := writeFrame(&buffer, []byte("{}")); err != nil { t.Fatal(err) }
    raw, err := readFrame(&buffer)
    if err != nil || string(raw) != "{}" { t.Fatal("frame mismatch") }
}

func TestWrongResponseIDDisconnects(t *testing.T) {
    local, remote := net.Pipe()
    defer remote.Close()
    client := &Client{conn: local}
    go func() {
        readFrame(remote)
        raw, _ := json.Marshal(protocol.Response{RequestId:"different",Result: pointer(json.RawMessage("{}"))})
        writeFrame(remote, raw)
    }()
    if err := client.Call(context.Background(), protocol.MethodHealth, protocol.EmptyParams{}, "EmptyParams", &protocol.EmptyParams{}); err != ErrProtocol { t.Fatalf("unexpected error: %v",err) }
}

func TestCancellationClosesOutstandingRead(t *testing.T) {
    local, remote := net.Pipe()
    defer remote.Close()
    client := &Client{conn: local}
    go func() { readFrame(remote) }()
    ctx, cancel := context.WithTimeout(context.Background(), 10 * time.Millisecond)
    defer cancel()
    if err := client.Call(ctx, protocol.MethodHealth, protocol.EmptyParams{}, "EmptyParams", &protocol.EmptyParams{}); err == nil { t.Fatal("unresponsive peer accepted") }
}

func TestInvalidTypedResultDisconnects(t *testing.T) {
    local, remote := net.Pipe()
    defer remote.Close()
    client := &Client{conn:local}
    go func() {
        raw, _ := readFrame(remote)
        var request protocol.Request
        json.Unmarshal(raw, &request)
        result := json.RawMessage(`{"entries":"invalid"}`)
        raw, _ = json.Marshal(protocol.Response{RequestId:request.RequestId,Result:&result})
        writeFrame(remote, raw)
    }()
    err := client.Call(context.Background(), protocol.MethodHistoryList, protocol.HistoryPageParams{Limit:20}, "HistoryResult", &protocol.HistoryResult{})
    if err == nil { t.Fatal("invalid result accepted") }
    if _, err = local.Write([]byte("x")); err == nil { t.Fatal("connection retained after protocol failure") }
}

func pointer[T any](value T) *T { return &value }
