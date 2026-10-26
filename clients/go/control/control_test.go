package control

import (
    "bytes"
    "encoding/binary"
    "strings"
    "testing"
    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)
func TestPrivateControlBoundsDirectionAndBigEndian(t *testing.T) {
    message, err := Message("stop-1", protocol.SurfaceControlTypeStopped, protocol.SurfaceStopped{SurfaceSessionId:"session-1"})
    if err != nil { t.Fatal(err) }
    var data bytes.Buffer
    if err = Write(&data, message, ChildToParent); err != nil { t.Fatal(err) }
    raw := append([]byte(nil), data.Bytes()...)
    if int(binary.BigEndian.Uint32(raw[:4])) != len(raw)-4 { t.Fatal("not BE4") }
    got, err := Read(&data, ChildToParent)
    if err != nil || got.RequestId != message.RequestId { t.Fatal("roundtrip", err) }
    if _, err = Read(bytes.NewReader(raw), ParentToChild); err == nil { t.Fatal("wrong direction") }
    for _, bad := range [][]byte{{0,0,0,0},{0,1,0,1},{0,0,0,2,'{'}} {
        if _, err = Read(bytes.NewReader(bad), ChildToParent); err == nil { t.Fatal("invalid frame") }
    }
}
func TestUnrequestedBodiesAndWrongVersionsAreRejected(t *testing.T) {
    message, _ := Message("stop-1", protocol.SurfaceControlTypeStopped, map[string]string{"surface_session_id":"session-1","body":"private"})
    if _, err := Encode(message, ChildToParent); err == nil { t.Fatal("body accepted") }
    message, _ = Message("stop-1", protocol.SurfaceControlTypeStopped, protocol.SurfaceStopped{SurfaceSessionId:"session-1"})
    message.ProtocolVersion.Minor++
    if _, err := Encode(message, ChildToParent); err == nil { t.Fatal("version accepted") }
    build := protocol.SurfaceBuildIdentity{ComponentVersion:"0.2.0-alpha.5",SourceRevision:strings.Repeat("a",40),BuildId:"0.2.0-alpha.5@"+strings.Repeat("a",40),Target:"x86_64-pc-windows-msvc"}
    if !SameBuild(build,build) { t.Fatal("valid build") }
    changed := build; changed.SourceRevision = strings.Repeat("b",40)
    if SameBuild(changed,build) { t.Fatal("source mismatch") }
    changed = build; changed.BuildId = "not compiled identity"
    if SameBuild(changed,changed) { t.Fatal("inconsistent identity") }
}
