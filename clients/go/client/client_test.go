package client

import (
    "bytes"
    "context"
    "encoding/json"
    "fmt"
    "io"
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

func TestNegotiatedFrameLimitAfterRealHandshake(t *testing.T) {
    request := protocol.Request{RequestId: "2", Method: protocol.MethodHealth, Params: json.RawMessage(`{}`)}
    raw, err := protocol.Encode("Request", request)
    if err != nil { t.Fatal(err) }
    for _, limit := range []uint32{1, uint32(len(raw)-1), uint32(len(raw)), protocol.MaxFrameBytes} {
        t.Run(fmt.Sprint(limit), func(t *testing.T) {
            local, remote := net.Pipe()
            defer remote.Close()
            ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
            defer cancel()
            observed := make(chan error, 1)
            go func() {
                defer remote.Close()
                incoming, failure := readFrame(remote)
                if failure != nil { observed <- failure; return }
                var helloRequest protocol.Request
                if failure = protocol.Decode("Request", incoming, &helloRequest); failure != nil { observed <- failure; return }
                result, failure := protocol.Encode("HelloResult", protocol.HelloResult{
                    SessionId: "synthetic-session", ProtocolVersion: protocol.Version, CoreVersion: "0.2.0-alpha.1",
                    Permissions: []protocol.Permission{}, State: protocol.RuntimeState{ServiceInstanceId: "synthetic-host"}, MaxFrameBytes: limit,
                })
                if failure != nil { observed <- failure; return }
                response, failure := protocol.Encode("Response", protocol.Response{RequestId: helloRequest.RequestId, Result: pointer(json.RawMessage(result))})
                if failure != nil { observed <- failure; return }
                if failure = writeFrame(remote, response); failure != nil { observed <- failure; return }
                if uint32(len(raw)) > limit {
                    var single [1]byte
                    count, failure := remote.Read(single[:])
                    if count != 0 || failure != io.EOF { observed <- fmt.Errorf("oversized request wrote %d bytes: %v", count, failure); return }
                    observed <- nil
                    return
                }
                incoming, failure = readFrame(remote)
                if failure != nil { observed <- failure; return }
                if !bytes.Equal(incoming, raw) { observed <- fmt.Errorf("negotiated request differs: %s", incoming); return }
                response, failure = protocol.Encode("Response", protocol.Response{RequestId: "2", Result: pointer(json.RawMessage(`{}`))})
                if failure == nil { failure = writeFrame(remote, response) }
                observed <- failure
            }()
            client, failure := handshake(ctx, local, protocol.HelloParams{ProtocolVersion: protocol.Version,
                ClientVersion: "0.2.0-alpha.1", ClientRole: protocol.ClientRoleSurface, Capabilities: []string{}, RequiredCapabilities: []string{}})
            if failure != nil { t.Fatal(failure) }
            defer client.Close()
            if client.Hello().MaxFrameBytes != limit { t.Fatal("negotiated limit was lost") }
            failure = client.Call(ctx, protocol.MethodHealth, protocol.EmptyParams{}, "EmptyParams", &protocol.EmptyParams{})
            if uint32(len(raw)) > limit {
                if failure != ErrProtocol { t.Fatalf("oversized request: %v", failure) }
            } else if failure != nil { t.Fatal(failure) }
            select {
            case failure = <-observed:
                if failure != nil { t.Fatal(failure) }
            case <-ctx.Done():
                t.Fatal("peer did not finish the bounded handshake/frame observation")
            }
        })
    }
}
