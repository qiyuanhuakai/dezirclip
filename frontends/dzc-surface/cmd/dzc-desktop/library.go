package main

import (
	"context"
	"encoding/base64"
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

func (l *Library) Platform(ctx context.Context) (json.RawMessage, error) {
	var result protocol.DesktopPlatformReport
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopPlatformProbe, struct{}{}, "DesktopPlatformReport", &result)
	return result.Report, err
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

func (l *Library) OpenEntry(ctx context.Context, params protocol.ExternalOpenParams) error {
	if params.ServiceInstanceId != l.owner.Hello().State.ServiceInstanceId || params.Id <= 0 {
		return client.ErrProtocol
	}
	var result protocol.OperationResult
	if err := l.owner.DesktopCall(ctx, protocol.MethodDesktopHistoryOpen, params, "OperationResult", &result); err != nil {
		return err
	}
	if !result.Performed {
		return client.ErrProtocol
	}
	return nil
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

// MutateEntry uses the identity and privacy epoch of the row the user acted on.
func (l *Library) MutateEntry(ctx context.Context, params protocol.DesktopMutationParams) (int64, error) {
	if params.ServiceInstanceId != l.owner.Hello().State.ServiceInstanceId || params.Id == 0 {
		return 0, client.ErrProtocol
	}
	switch params.Action {
	case "pin", "tags", "edit", "delete":
	default:
		return 0, client.ErrProtocol
	}
	var result protocol.DesktopMutationResult
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopHistoryMutate, params, "DesktopMutationResult", &result)
	if err == nil && (!result.Performed || result.Id == 0) {
		return 0, client.ErrProtocol
	}
	return result.Id, err
}

func (l *Library) AddManual(ctx context.Context, content string, params protocol.ManualEntryParams) (int64, error) {
	if len(content) > 32*1024*1024 {
		return 0, errors.New("ManualEntryTooLarge")
	}
	params.ServiceInstanceId = l.owner.Hello().State.ServiceInstanceId
	host, err := l.owner.DesktopConnection(ctx)
	if err != nil {
		return 0, err
	}
	defer host.Close()
	var begin protocol.CaptureBeginResult
	if err := host.Call(ctx, protocol.MethodDesktopHistoryManualBegin, params, "CaptureBeginResult", &begin); err != nil {
		return 0, err
	}
	if begin.CaptureId == nil || begin.State.ServiceInstanceId != params.ServiceInstanceId || begin.State.PrivacyEpoch != params.ExpectedEpoch {
		return 0, client.ErrProtocol
	}
	const chunkSize = 192 * 1024
	for offset := 0; offset < len(content); offset += chunkSize {
		chunk := content[offset:min(offset+chunkSize, len(content))]
		write := protocol.CaptureWriteParams{CaptureId: *begin.CaptureId, Offset: uint32(offset), DataBase64: base64.StdEncoding.EncodeToString([]byte(chunk))}
		var result protocol.OperationResult
		if err := host.Call(ctx, protocol.MethodDesktopCaptureWrite, write, "OperationResult", &result); err != nil {
			return 0, err
		}
		if !result.Performed {
			return 0, client.ErrProtocol
		}
	}
	var saved protocol.DesktopMutationResult
	if err := host.Call(ctx, protocol.MethodDesktopHistoryManualFinish, protocol.ManualEntryFinish{CaptureId: *begin.CaptureId}, "DesktopMutationResult", &saved); err != nil {
		return 0, err
	}
	if !saved.Performed || saved.Id <= 0 {
		return 0, client.ErrProtocol
	}
	return saved.Id, nil
}

func (l *Library) TagColors(ctx context.Context, epoch uint64) (json.RawMessage, error) {
	params := protocol.LibraryStateParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	var colors protocol.LibraryColors
	err := l.snapshot(ctx, protocol.MethodDesktopTagsColors, params, "LibraryColors", &colors, epoch)
	return colors.Values, err
}

func (l *Library) EditTag(ctx context.Context, action string, name string, newName string, epoch uint64) error {
	instance := l.owner.Hello().State.ServiceInstanceId
	var method protocol.Method
	var params any = protocol.LibraryTagParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Name: name}
	switch action {
	case "create":
		method = protocol.MethodDesktopTagsCreate
	case "remove":
		method = protocol.MethodDesktopTagsRemove
	case "rename":
		method = protocol.MethodDesktopTagsRename
		params = protocol.LibraryRenameParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Name: name, NewName: newName}
	default:
		return client.ErrProtocol
	}
	return l.edit(ctx, method, params)
}

func (l *Library) edit(ctx context.Context, method protocol.Method, params any) error {
	var result protocol.OperationResult
	err := l.owner.DesktopCall(ctx, method, params, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

func (l *Library) ClearHistory(ctx context.Context, epoch uint64) error {
	return l.edit(ctx, protocol.MethodDesktopHistoryClear, protocol.LibraryStateParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch,
	})
}

func (l *Library) SetWindowPinned(ctx context.Context, pinned bool, epoch uint64) error {
	return l.edit(ctx, protocol.MethodDesktopWindowPin, protocol.WindowPinParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Pinned: pinned,
	})
}

func (l *Library) ReorderPins(ctx context.Context, pairs [][2]int64, epoch uint64) error {
	orders := make([]protocol.LibraryPinOrder, len(pairs))
	for i, pair := range pairs {
		orders[i] = protocol.LibraryPinOrder{Id: pair[0], Order: pair[1]}
	}
	return l.edit(ctx, protocol.MethodDesktopHistoryReorder, protocol.LibraryOrderParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Orders: orders,
	})
}

func (l *Library) DeleteTagEntries(ctx context.Context, params protocol.LibraryTagBatchParams) (uint64, error) {
	params.ServiceInstanceId = l.owner.Hello().State.ServiceInstanceId
	return l.deleteEntries(ctx, protocol.MethodDesktopTagsDeleteEntries, params)
}

func (l *Library) DeleteTagWithContents(ctx context.Context, name string, epoch uint64) (uint64, error) {
	return l.deleteEntries(ctx, protocol.MethodDesktopTagsDeleteContents, protocol.LibraryTagParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Name: name,
	})
}

func (l *Library) deleteEntries(ctx context.Context, method protocol.Method, params any) (uint64, error) {
	var result protocol.LibraryDeleted
	err := l.owner.DesktopCall(ctx, method, params, "LibraryDeleted", &result)
	if err == nil && result.CleanupFailed {
		err = errors.New("History was deleted, but attachment cleanup failed")
	}
	return result.DeletedCount, err
}

func (l *Library) Preferences(ctx context.Context) (json.RawMessage, error) {
	status, err := l.Status(ctx)
	if err != nil {
		return nil, err
	}
	params := protocol.LibraryStateParams{ServiceInstanceId: status.State.ServiceInstanceId, ExpectedEpoch: status.State.PrivacyEpoch}
	var preferences protocol.DesktopPreferences
	if err := l.snapshot(ctx, protocol.MethodDesktopSettingsPreferences, params, "DesktopPreferences", &preferences, params.ExpectedEpoch); err != nil {
		return nil, err
	}
	return preferences.Values, nil
}

func (l *Library) PasteQueue(ctx context.Context, epoch uint64) ([]int64, error) {
	var result protocol.QueueState
	params := protocol.LibraryStateParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopQueueGet, params, "QueueState", &result)
	return result.Ids, err
}

func (l *Library) SetPasteQueue(ctx context.Context, epoch uint64, ids []int64) error {
	for _, id := range ids {
		if id <= 0 {
			return errors.New("SessionQueueUnavailable")
		}
	}
	return l.edit(ctx, protocol.MethodDesktopQueueSet, protocol.QueueReplaceParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Ids: ids,
	})
}

func (l *Library) SetSetting(ctx context.Context, key, value string, epoch uint64) error {
	instance := l.owner.Hello().State.ServiceInstanceId
	var result protocol.OperationResult
	var err error
	switch key {
	case "app.text", "app.rich_text", "app.image", "app.video", "app.code", "app.url":
		params := protocol.ApplicationSelection{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
		err = l.owner.DesktopCall(ctx, protocol.MethodDesktopApplicationsSelect, params, "OperationResult", &result)
	case "app.theme", "app.color_mode", "app.language", "app.compact_mode", "app.show_app_border",
		"app.show_scroll_top_button", "app.show_search_box", "app.arrow_key_selection", "app.silent_start", "app.hide_tray_icon", "app.sound_enabled", "app.sound_paste_enabled", "app.sound_volume",
		"app.clipboard_item_font_size", "app.clipboard_tag_font_size", "app.surface_opacity", "app.font_main", "app.font_mono",
		"app.custom_background", "app.custom_background_opacity", "app.emoji_panel_enabled", "app.emoji_panel_tab", "app.emoji_favorites":
		params := protocol.ViewSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
		err = l.owner.DesktopCall(ctx, protocol.MethodDesktopSettingsViewSet, params, "OperationResult", &result)
	case "app.sequential_mode", "app.delete_after_paste", "app.paste_method":
		params := protocol.QueueSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
		err = l.owner.DesktopCall(ctx, protocol.MethodDesktopQueueConfigure, params, "OperationResult", &result)
	case "app.persistent", "app.deduplicate", "app.capture_files", "app.capture_rich_text", "app.privacy_protection",
		"app.privacy_protection_kinds", "app.privacy_protection_custom_rules", "app.excluded_source_apps", "app.ocr_enabled":
		params := protocol.CaptureSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
		err = l.owner.DesktopCall(ctx, protocol.MethodDesktopSettingsCaptureSet, params, "OperationResult", &result)
	default:
		return errors.New("UnsupportedDesktopSetting")
	}
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
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
