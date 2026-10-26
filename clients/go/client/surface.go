package client

import (
    "context"
    "encoding/json"
    "errors"
    "sync"
    "github.com/qiyuanhuakai/dezirclip/clients/go/control"
    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

var ErrSurfaceState = errors.New("surface state changed; refresh and select again")
func sameSurfaceState(a,b protocol.RuntimeState) bool {
    return a.ServiceInstanceId == b.ServiceInstanceId && a.PrivacyEpoch == b.PrivacyEpoch && a.HistoryRevision == b.HistoryRevision
}
func validateSurface(hello protocol.HelloResult, capability protocol.SurfacePageCapability, expected protocol.SurfaceBuildIdentity) error {
    if hello.CoreVersion != expected.ComponentVersion || len(hello.Permissions) != 1 || hello.Permissions[0] != protocol.PermissionHistoryRead ||
        capability.Version != 1 || capability.MaxPageSize != 100 || !control.SameBuild(capability.Build, expected) { return ErrProtocol }
    return nil
}
// DialSurface never reads a Desktop control file or requests action/body grants.
func DialSurface(ctx context.Context, address, grant string, expected protocol.SurfaceBuildIdentity) (*Client,error) {
    client, err := Dial(ctx,address,protocol.HelloParams{ProtocolVersion:protocol.Version,ClientVersion:expected.ComponentVersion,ClientRole:protocol.ClientRoleSurface,
        Capabilities:[]string{},RequiredCapabilities:[]string{"surface.history.page"},Grant:&grant})
    if err != nil { return nil,err }
    var capabilities protocol.CapabilityReport
    if err = client.Call(ctx,protocol.MethodSurfaceCapabilities,protocol.EmptyParams{},"CapabilityReport",&capabilities); err != nil { client.Close();return nil,err }
    var platform map[string]json.RawMessage
    if json.Unmarshal(capabilities.Platform,&platform) != nil { client.Close();return nil,ErrProtocol }
    var capability protocol.SurfacePageCapability
    if protocol.Decode("SurfacePageCapability",platform["surface_page_v1"],&capability) != nil || validateSurface(client.hello,capability,expected) != nil {
        client.Close();return nil,ErrProtocol
    }
    return client,nil
}
func (client *Client) SurfacePage(ctx context.Context, request protocol.SurfacePageParams) (protocol.SurfacePage,error) {
    var page protocol.SurfacePage
    if request.ServiceInstanceId != client.hello.State.ServiceInstanceId { return page,ErrHostChanged }
    if err := client.Call(ctx,protocol.MethodSurfaceHistoryPage,request,"SurfacePage",&page); err != nil { return page,err }
    expected := protocol.RuntimeState{ServiceInstanceId:request.ServiceInstanceId,PrivacyEpoch:request.ExpectedEpoch,HistoryRevision:request.ExpectedHistoryRevision}
    if !sameSurfaceState(page.State,expected) || len(page.Entries)>int(request.Limit) ||
        (page.HasMore && (len(page.Entries)==0 || page.NextOffset==nil || *page.NextOffset!=request.Offset+uint32(len(page.Entries)))) ||
        (!page.HasMore && page.NextOffset!=nil) { client.Close();return protocol.SurfacePage{},ErrSurfaceState }
    return page,nil
}

// SurfaceQueries fences UI delivery only. Host permissions remain authoritative.
type SurfaceQueries struct { mu sync.Mutex; session,display string; state protocol.RuntimeState; generation,sequence uint64; active bool }
type SurfaceQuery struct { SessionID,DisplayID string; State protocol.RuntimeState; generation,sequence uint64 }
func (view *SurfaceQueries) Reset(session,display string,state protocol.RuntimeState) {
    view.mu.Lock();defer view.mu.Unlock();view.generation++;view.sequence=0;view.session=session;view.display=display;view.state=state;view.active=session!=""&&display!=""
}
func (view *SurfaceQueries) Cancel() { view.mu.Lock();defer view.mu.Unlock();view.generation++;view.active=false }
func (view *SurfaceQueries) Begin() (SurfaceQuery,error) {
    view.mu.Lock();defer view.mu.Unlock();if !view.active{return SurfaceQuery{},ErrSurfaceState};view.sequence++
    return SurfaceQuery{SessionID:view.session,DisplayID:view.display,State:view.state,generation:view.generation,sequence:view.sequence},nil
}
func (view *SurfaceQueries) Accept(query SurfaceQuery,page protocol.SurfacePage) bool {
    view.mu.Lock();defer view.mu.Unlock()
    return view.active && query.generation==view.generation && query.sequence==view.sequence && query.SessionID==view.session && query.DisplayID==view.display &&
        sameSurfaceState(query.State,view.state) && sameSurfaceState(page.State,view.state)
}
