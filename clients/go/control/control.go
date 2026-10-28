// Package control carries only metadata on private inherited parent/child pipes.
package control

import (
    "encoding/binary"
    "encoding/json"
    "errors"
    "io"
    "regexp"

    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

const MaxFrameBytes = 64 * 1024
type Direction uint8
const ( ParentToChild Direction = iota; ChildToParent )
var ErrInvalid = errors.New("invalid private surface control")
var sourcePattern = regexp.MustCompile(`^[a-f0-9]{40}$`)

func check(message protocol.SurfaceControlMessage, direction Direction) error {
    if message.ProtocolVersion != protocol.Version || direction > ChildToParent { return ErrInvalid }
    parent := message.Type == protocol.SurfaceControlTypeInitialize || message.Type == protocol.SurfaceControlTypeShow ||
        message.Type == protocol.SurfaceControlTypeHide || message.Type == protocol.SurfaceControlTypePasteResult || message.Type == protocol.SurfaceControlTypeShutdown
    if parent != (direction == ParentToChild) { return ErrInvalid }
    return nil
}
func Encode(message protocol.SurfaceControlMessage, direction Direction) ([]byte, error) {
    if err := check(message, direction); err != nil { return nil, err }
    raw, err := protocol.Encode("SurfaceControlMessage", message)
    if err != nil || len(raw) == 0 || len(raw) > MaxFrameBytes { return nil, ErrInvalid }
    return raw, nil
}
func Decode(raw []byte, direction Direction) (protocol.SurfaceControlMessage, error) {
    var message protocol.SurfaceControlMessage
    if len(raw) == 0 || len(raw) > MaxFrameBytes { return message, ErrInvalid }
    if err := protocol.Decode("SurfaceControlMessage", raw, &message); err != nil { return message, ErrInvalid }
    return message, check(message, direction)
}
func Message(id string, kind protocol.SurfaceControlType, payload any) (protocol.SurfaceControlMessage, error) {
    raw, err := json.Marshal(payload)
    return protocol.SurfaceControlMessage{ProtocolVersion:protocol.Version,RequestId:id,Type:kind,Payload:raw}, err
}
func Read(reader io.Reader, direction Direction) (protocol.SurfaceControlMessage, error) {
    var header [4]byte
    if _, err := io.ReadFull(reader, header[:]); err != nil { return protocol.SurfaceControlMessage{}, err }
    length := binary.BigEndian.Uint32(header[:])
    if length == 0 || length > MaxFrameBytes { return protocol.SurfaceControlMessage{}, ErrInvalid }
    raw := make([]byte, length)
    if _, err := io.ReadFull(reader, raw); err != nil { return protocol.SurfaceControlMessage{}, err }
    return Decode(raw, direction)
}
func Write(writer io.Writer, message protocol.SurfaceControlMessage, direction Direction) error {
    raw, err := Encode(message, direction)
    if err != nil { return err }
    bytes := make([]byte, 4 + len(raw)); binary.BigEndian.PutUint32(bytes, uint32(len(raw))); copy(bytes[4:], raw)
    for len(bytes) > 0 {
        written, err := writer.Write(bytes)
        if err != nil { return err }
        if written <= 0 { return io.ErrShortWrite }
        bytes = bytes[written:]
    }
    return nil
}
func SameBuild(actual, expected protocol.SurfaceBuildIdentity) bool {
    return actual == expected && sourcePattern.MatchString(actual.SourceRevision) && actual.ComponentVersion != "" && actual.Target != "" &&
        actual.BuildId == actual.ComponentVersion + "@" + actual.SourceRevision
}
