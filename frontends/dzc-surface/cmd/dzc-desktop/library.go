package main

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Library is the shared React window's authenticated view of the Rust Host.
type Library struct {
	owner *client.Client
}

type hostedEntry struct {
	protocol.DesktopEntry
	Instance string `json:"host_instance_id"`
}

func (l *Library) Status(ctx context.Context) (protocol.ServiceStatus, error) {
	var status protocol.ServiceStatus
	err := l.owner.DesktopCall(ctx, protocol.MethodServiceStatus, struct{}{}, "ServiceStatus", &status)
	return status, err
}

func (l *Library) SetPrivacy(ctx context.Context, params protocol.PrivacySetParams) (protocol.ServiceStatus, error) {
	if !params.Active && params.DurationMs != nil {
		return protocol.ServiceStatus{}, errors.New("PrivacyDurationInvalid")
	}
	var privacy protocol.PrivacyState
	if err := l.owner.DesktopCall(ctx, protocol.MethodPrivacySet, params, "PrivacyState", &privacy); err != nil {
		return protocol.ServiceStatus{}, err
	}
	status, err := l.Status(ctx)
	if err == nil && status.Privacy.PrivacyEpoch != privacy.PrivacyEpoch {
		return protocol.ServiceStatus{}, errors.New("PrivacyEpochChanged")
	}
	return status, err
}

// The descriptor and its chunks must be read on the same authenticated stream.
func (l *Library) snapshot(ctx context.Context, method protocol.Method, params any, schema string, result any, epoch uint64) error {
	host, err := l.owner.DesktopConnection(ctx)
	if err != nil {
		return err
	}
	defer host.Close()
	var body protocol.BodySnapshot
	if err := host.Call(ctx, method, params, "BodySnapshot", &body); err != nil {
		return err
	}
	if body.State.PrivacyEpoch != epoch {
		return errors.New("PrivacyEpochChanged")
	}
	return host.ReadSnapshot(ctx, body, schema, result)
}

func (l *Library) Page(ctx context.Context, params protocol.DesktopPageParams, epoch uint64) ([]hostedEntry, error) {
	var page protocol.DesktopEntries
	if err := l.snapshot(ctx, protocol.MethodDesktopHistoryPage, params, "DesktopEntries", &page, epoch); err != nil {
		return nil, err
	}
	return l.rows(page.Entries), nil
}

func (l *Library) rows(entries []protocol.DesktopEntry) []hostedEntry {
	rows := make([]hostedEntry, len(entries))
	for i, entry := range entries {
		rows[i] = hostedEntry{entry, l.owner.Hello().State.ServiceInstanceId}
	}
	return rows
}

type tagPage struct {
	Items      []hostedEntry `json:"items"`
	Total      uint64        `json:"total"`
	HasMore    bool          `json:"has_more"`
	NextOffset *uint32       `json:"next_offset"`
}

func (l *Library) TagPage(ctx context.Context, params protocol.DesktopTagPageParams) (tagPage, error) {
	params.ServiceInstanceId = l.owner.Hello().State.ServiceInstanceId
	var page protocol.DesktopTagItemsPage
	if err := l.snapshot(ctx, protocol.MethodDesktopTagsPage, params, "DesktopTagItemsPage", &page, params.ExpectedEpoch); err != nil {
		return tagPage{}, err
	}
	return tagPage{l.rows(page.Items), page.Total, page.HasMore, page.NextOffset}, nil
}

func (l *Library) Entry(ctx context.Context, params protocol.DesktopEntryParams) (hostedEntry, error) {
	return l.entry(ctx, protocol.MethodDesktopHistoryGet, params)
}

func (l *Library) Summary(ctx context.Context, params protocol.DesktopEntryParams) (hostedEntry, error) {
	params.Reveal = false
	params.ThumbnailDimension = nil
	return l.entry(ctx, protocol.MethodDesktopHistorySummary, params)
}

func (l *Library) TagCounts(ctx context.Context, epoch uint64) (json.RawMessage, error) {
	params := protocol.LibraryStateParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	var counts protocol.LibraryCounts
	if err := l.snapshot(ctx, protocol.MethodDesktopTagsCounts, params, "LibraryCounts", &counts, epoch); err != nil {
		return nil, err
	}
	return counts.Values, nil
}

func (l *Library) entry(ctx context.Context, method protocol.Method, params protocol.DesktopEntryParams) (hostedEntry, error) {
	instance := l.owner.Hello().State.ServiceInstanceId
	if (params.Id < 0 || params.ServiceInstanceId != "") && params.ServiceInstanceId != instance {
		return hostedEntry{}, errors.New("ClipboardHostInstanceChanged")
	}
	params.ServiceInstanceId = instance
	var entry protocol.DesktopEntry
	if err := l.snapshot(ctx, method, params, "DesktopEntry", &entry, params.ExpectedEpoch); err != nil {
		return hostedEntry{}, err
	}
	if entry.Id != params.Id {
		return hostedEntry{}, client.ErrProtocol
	}
	return hostedEntry{entry, instance}, nil
}
