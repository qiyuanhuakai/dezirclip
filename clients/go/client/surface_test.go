package client

import (
    "context"
    "encoding/json"
    "net"
    "strings"
    "testing"
    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)
func TestSurfacePageRejectsChangedStateAndFalsePagination(t *testing.T) {
    state:=protocol.RuntimeState{ServiceInstanceId:"host-1",PrivacyEpoch:4,HistoryRevision:7}
    for _, bad:=range []bool{false,true} {
        local,remote:=net.Pipe()
        client:=&Client{conn:local,hello:protocol.HelloResult{State:state}}
        completed:=make(chan error,1)
        go func(){
            defer remote.Close()
            raw,err:=readFrame(remote);if err!=nil{completed<-err;return}
            var request protocol.Request;if err=json.Unmarshal(raw,&request);err!=nil{completed<-err;return}
            page:=protocol.SurfacePage{Entries:[]protocol.EntrySummary{},State:state}
            if bad { page.HasMore=true } else { page.State.PrivacyEpoch++ }
            payload,_:=json.Marshal(page);result:=json.RawMessage(payload)
            response,_:=json.Marshal(protocol.Response{RequestId:request.RequestId,Result:&result})
            completed<-writeFrame(remote,response)
        }()
        _,err:=client.SurfacePage(context.Background(),protocol.SurfacePageParams{ServiceInstanceId:state.ServiceInstanceId,ExpectedEpoch:state.PrivacyEpoch,ExpectedHistoryRevision:state.HistoryRevision,Limit:10})
        if err!=ErrSurfaceState{t.Fatalf("invalid state/page accepted: %v",err)}
        if err=<-completed;err!=nil{t.Fatal(err)}
        client.Close()
    }
}
func TestSurfaceNegotiationRequiresReadOnlyGrantAndExactCompiledBuild(t *testing.T) {
    build:=protocol.SurfaceBuildIdentity{ComponentVersion:"0.2.0-alpha.5",SourceRevision:strings.Repeat("a",40),BuildId:"0.2.0-alpha.5@"+strings.Repeat("a",40),Target:"x86_64-unknown-linux-gnu"}
    hello:=protocol.HelloResult{CoreVersion:build.ComponentVersion,Permissions:[]protocol.Permission{protocol.PermissionHistoryRead}}
    capability:=protocol.SurfacePageCapability{Version:1,MaxPageSize:100,Build:build}
    if validateSurface(hello,capability,build)!=nil {t.Fatal("valid negotiation")}
    hello.Permissions=append(hello.Permissions,protocol.PermissionClipboardPaste)
    if validateSurface(hello,capability,build)==nil {t.Fatal("overprivileged helper")}
    hello.Permissions=hello.Permissions[:1];capability.Build.SourceRevision=strings.Repeat("b",40)
    if validateSurface(hello,capability,build)==nil {t.Fatal("different source")}
}
func TestSupersededQueryDisplayHostAndPrivacyNeverDeliverOldMetadata(t *testing.T) {
    state:=protocol.RuntimeState{ServiceInstanceId:"host-1",PrivacyEpoch:4,HistoryRevision:7}
    page:=protocol.SurfacePage{State:state};var view SurfaceQueries
    view.Reset("child-1","display-1",state)
    first,_:=view.Begin();second,_:=view.Begin()
    if view.Accept(first,page)||!view.Accept(second,page){t.Fatal("query generation")}
    view.Reset("child-1","display-2",state)
    if view.Accept(second,page){t.Fatal("old display")}
    current,_:=view.Begin();changed:=page;changed.State.PrivacyEpoch++
    if view.Accept(current,changed){t.Fatal("old epoch")}
    changed=page;changed.State.ServiceInstanceId="host-2"
    if view.Accept(current,changed){t.Fatal("new owner")}
    changed=page;changed.State.HistoryRevision++
    if view.Accept(current,changed){t.Fatal("changed revision")}
    view.Cancel();if view.Accept(current,page){t.Fatal("hidden display")}
}
