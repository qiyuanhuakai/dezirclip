package client

import (
    "context"
    "errors"
    "github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

var ErrHostChanged = errors.New("entry belongs to another host instance")

// EntryReference captures the instance when a summary is received. Reconnecting
// never silently assigns a previous negative ID to the new Host.
type EntryReference struct { ID int64; InstanceID string }
func (client *Client) Reference(entry protocol.EntrySummary) EntryReference {
    return EntryReference{ID:entry.Id,InstanceID:client.hello.State.ServiceInstanceId}
}
func (client *Client) Copy(ctx context.Context, entry EntryReference) error {
    if entry.ID < 0 && entry.InstanceID != client.hello.State.ServiceInstanceId { return ErrHostChanged }
    var instance *string
    if entry.ID < 0 { instance = &entry.InstanceID }
    var result protocol.OperationResult
    return client.Call(ctx,protocol.MethodSurfaceCopy,protocol.ClipboardCopyParams{Id:entry.ID,ServiceInstanceId:instance},"OperationResult",&result)
}

// Cursor only contains metadata. A new instance, privacy epoch, dropped ring
// window or backwards revision invalidates the caller's entire cached view.
type Cursor struct { State protocol.RuntimeState; Ready bool }
func (cursor *Cursor) Observe(events protocol.EventsResult) bool {
    state:=events.State
    refresh:=!cursor.Ready || events.ResyncRequired || cursor.State.ServiceInstanceId!=state.ServiceInstanceId || cursor.State.PrivacyEpoch!=state.PrivacyEpoch ||
        state.HistoryRevision<cursor.State.HistoryRevision || state.EventSequence<cursor.State.EventSequence
    cursor.State=state;cursor.Ready=true
    return refresh
}
