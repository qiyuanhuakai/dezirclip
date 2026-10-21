package client

import (
    "context"
    "testing"
    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)
func TestReconnectCannotRelabelNegativeEntry(t *testing.T) {
    old:=&Client{hello:protocol.HelloResult{State:protocol.RuntimeState{ServiceInstanceId:"old"}}}
    reference:=old.Reference(protocol.EntrySummary{Id:-7})
    next:=&Client{hello:protocol.HelloResult{State:protocol.RuntimeState{ServiceInstanceId:"new"}}}
    if err:=next.Copy(context.Background(),reference);err!=ErrHostChanged {t.Fatalf("stale reference reached transport: %v",err)}
    if reference.InstanceID!="old" {t.Fatal("reference was relabeled")}
}
func TestMetadataCursorInvalidatesOnPrivacyAndInstanceEvenAtEqualEpoch(t *testing.T) {
    state:=protocol.RuntimeState{ServiceInstanceId:"first",HistoryRevision:1,PrivacyEpoch:2,EventSequence:3}
    cursor:=Cursor{}
    if !cursor.Observe(protocol.EventsResult{State:state}) {t.Fatal("initial snapshot was not requested")}
    state.EventSequence++
    if cursor.Observe(protocol.EventsResult{State:state}) {t.Fatal("ordinary metadata event invalidated stable identity")}
    state.ServiceInstanceId="second"
    if !cursor.Observe(protocol.EventsResult{State:state}) {t.Fatal("equal-epoch restart retained old entries")}
    state.PrivacyEpoch++
    if !cursor.Observe(protocol.EventsResult{State:state}) {t.Fatal("privacy change retained old view")}
    if !cursor.Observe(protocol.EventsResult{State:state,ResyncRequired:true}) {t.Fatal("dropped event window was not refreshed")}
}
