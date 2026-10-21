// Package client connects the same versioned protocol to either Host mode.
package client

import (
    "context"
    "encoding/binary"
    "encoding/json"
    "errors"
    "io"
    "net"
    "strconv"
    "sync"
    "time"

    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

var ErrProtocol = errors.New("invalid local protocol response")
type RemoteError struct { Code protocol.WireErrorCode }
func (err *RemoteError) Error() string { return string(err.Code) }

type Client struct { conn net.Conn; mu sync.Mutex; sequence uint64; hello protocol.HelloResult }

func Dial(ctx context.Context, address string, hello protocol.HelloParams) (*Client, error) {
    conn, err := dialLocal(ctx, address)
    if err != nil { return nil, err }
    return handshake(ctx, conn, hello)
}

func handshake(ctx context.Context, conn net.Conn, hello protocol.HelloParams) (*Client, error) {
    client := &Client{conn: conn}
    if err := client.Call(ctx, protocol.MethodHello, hello, "HelloResult", &client.hello); err != nil { conn.Close(); return nil, err }
    if client.hello.ProtocolVersion.Major != protocol.Version.Major || client.hello.MaxFrameBytes == 0 || client.hello.MaxFrameBytes > protocol.MaxFrameBytes { conn.Close(); return nil, ErrProtocol }
    return client, nil
}

func (client *Client) Close() error { return client.conn.Close() }
func (client *Client) Hello() protocol.HelloResult { return client.hello }

// Call serializes one bounded request/response. Context deadlines cover both
// writes and reads; callers reconnect and refresh state after disconnection.
func (client *Client) Call(ctx context.Context, method protocol.Method, params any, schema string, result any) error {
    client.mu.Lock()
    defer client.mu.Unlock()
    if err := ctx.Err(); err != nil { return err }
    deadline, ok := ctx.Deadline()
    if maximum := time.Now().Add(5 * time.Second); !ok || deadline.After(maximum) { deadline = maximum }
    if err := client.conn.SetDeadline(deadline); err != nil { return err }
    cancellationDone := make(chan struct{})
    stop := context.AfterFunc(ctx, func() { client.conn.SetDeadline(time.Now()); close(cancellationDone) })
    defer func() { if !stop() { <-cancellationDone } }()
    client.sequence++
    id := strconv.FormatUint(client.sequence, 10)
    paramBytes, err := json.Marshal(params)
    if err != nil { return protocol.ErrInvalidMessage }
    request := protocol.Request{RequestId: id, Method: method, Params: paramBytes}
    raw, err := protocol.Encode("Request", request)
    if err != nil { return err }
    limit := client.hello.MaxFrameBytes
    if limit == 0 { limit = protocol.MaxFrameBytes }
    if err = writeFrameLimited(client.conn, raw, limit); err != nil { client.conn.Close(); return err }
    raw, err = readFrame(client.conn)
    if err != nil { client.conn.Close(); return err }
    var response protocol.Response
    if err = protocol.Decode("Response", raw, &response); err != nil { client.conn.Close(); return err }
    if response.RequestId != id { client.conn.Close(); return ErrProtocol }
    if response.Error != nil { return &RemoteError{Code: response.Error.Code} }
    if response.Result == nil { client.conn.Close(); return ErrProtocol }
    if err = protocol.Decode(schema, *response.Result, result); err != nil { client.conn.Close(); return err }
    return nil
}

func readFrame(reader io.Reader) ([]byte, error) {
    var length uint32
    if err := binary.Read(reader, binary.BigEndian, &length); err != nil { return nil, err }
    if length == 0 || length > protocol.MaxFrameBytes { return nil, ErrProtocol }
    raw := make([]byte, length)
    _, err := io.ReadFull(reader, raw)
    return raw, err
}

func writeFrame(writer io.Writer, raw []byte) error {
    return writeFrameLimited(writer, raw, protocol.MaxFrameBytes)
}

func writeFrameLimited(writer io.Writer, raw []byte, limit uint32) error {
    if limit == 0 || limit > protocol.MaxFrameBytes || len(raw) == 0 || uint64(len(raw)) > uint64(limit) { return ErrProtocol }
    buffer := make([]byte, 4 + len(raw))
    binary.BigEndian.PutUint32(buffer, uint32(len(raw)))
    copy(buffer[4:], raw)
    for len(buffer) > 0 {
        count, err := writer.Write(buffer)
        if err != nil { return err }
        if count <= 0 { return io.ErrShortWrite }
        buffer = buffer[count:]
    }
    return nil
}
