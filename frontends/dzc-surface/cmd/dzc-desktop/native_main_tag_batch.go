package main

import (
	"context"
	"maps"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

const maxNativeTagSelectionIDs = 10_000

// In all mode, ids contains exclusions to the selected tag query.
type nativeTagSelection struct {
	all bool
	ids map[int64]struct{}
}

func (s nativeTagSelection) includes(id int64) bool {
	_, listed := s.ids[id]
	return id > 0 && listed != s.all
}

func (s *nativeTagSelection) toggle(id int64) bool {
	if id <= 0 {
		return false
	}
	if _, listed := s.ids[id]; listed {
		delete(s.ids, id)
		return true
	}
	if len(s.ids) >= maxNativeTagSelectionIDs {
		return false
	}
	if s.ids == nil {
		s.ids = make(map[int64]struct{})
	}
	s.ids[id] = struct{}{}
	return true
}

func (s nativeTagSelection) count(total uint64) uint64 {
	listed := uint64(len(s.ids))
	if s.all {
		return total - min(total, listed)
	}
	return listed
}

func (s nativeTagSelection) request(filter nativeHistoryFilter, state protocol.RuntimeState) protocol.LibraryTagBatchParams {
	ids := make([]int64, 0, len(s.ids))
	ids = append(ids, slices.Sorted(maps.Keys(s.ids))...)
	request := protocol.LibraryTagBatchParams{
		ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
		Name: filter.tag, Scope: "selected", Ids: ids,
	}
	if s.all {
		request.Scope, request.Query = "all", &filter.query
	}
	return request
}

type nativeTagBatch struct {
	enabled, failed bool
	generation      uint64
	selection       nativeTagSelection
	notice          string
	pending         *nativeTagBatchConfirmation
}

type nativeTagBatchConfirmation struct {
	open       bool
	generation uint64
	revision   uint64
	params     protocol.LibraryTagBatchParams
	message    string
}

func (n *nativeMain) activeTagBatch() *nativeTagBatch {
	if tags := n.tagLibrary; tags != nil && tags.batch.enabled {
		return &tags.batch
	}
	return nil
}

func (n *nativeMain) tagBatchReady(batch *nativeTagBatch) bool {
	return n.session != nil && n.activeTagBatch() == batch && !n.mutating && !n.failed &&
		n.history.filter.tag != "" && n.history.filter == n.historyFilter() &&
		batch.generation == n.history.generation
}

func (n *nativeMain) toggleTagBatchRow(batch *nativeTagBatch, id int64, generation uint64) {
	if !n.tagBatchReady(batch) || id <= 0 || generation != n.history.generation {
		return
	}
	batch.notice, batch.failed = "", false
	if !batch.selection.toggle(id) {
		batch.notice = strings.ReplaceAll(n.text("tag_selection_limit"), "{limit}", strconv.Itoa(maxNativeTagSelectionIDs))
		batch.failed = true
	}
}

func (n *nativeMain) selectTagBatchMatches(batch *nativeTagBatch) {
	if !n.tagBatchReady(batch) || n.reading || n.history.total == 0 {
		return
	}
	batch.selection = nativeTagSelection{all: !batch.selection.all || len(batch.selection.ids) != 0}
	batch.notice, batch.failed = "", false
}

func (n *nativeMain) tagBatchKey(batch *nativeTagBatch, event ui.InputEvent) bool {
	if event.Kind != ui.InputKeyDown || n.hasHistoryDialog() {
		return false
	}
	if event.Mods == ui.Ctrl && event.Key == ui.KeyA {
		n.selectTagBatchMatches(batch)
		return true
	}
	if event.Mods == 0 && event.Key == ui.KeyDelete {
		n.confirmTagBatch(batch)
		return true
	}
	return false
}

func (n *nativeMain) tagBatchToolbar(c *ui.Context, kit uikit.Kit, tags *nativeTagLibrary) {
	batch := &tags.batch
	generation := n.history.generation
	if batch.enabled && batch.generation != generation {
		if batch.selection.all || len(batch.selection.ids) > 0 {
			batch.notice, batch.failed = n.text("tag_selection_changed"), false
		}
		batch.selection, batch.generation = nativeTagSelection{}, generation
	}
	ui.Column(c).FillWidth().Padding(0, 8).Gap(4).Children(func() {
		ui.Row(c).FillWidth().Gap(6).AlignItems(ui.Center).Wrap().Children(func() {
			if !batch.enabled {
				return
			}
			ready := n.tagBatchReady(batch) && !n.reading
			count := batch.selection.count(n.history.total)
			allLabel := n.text("tag_select_all")
			if batch.selection.all && len(batch.selection.ids) == 0 {
				allLabel = n.text("tag_clear_selection")
			} else if n.history.filter.query != "" {
				allLabel = n.text("tag_select_all_matches")
			}
			if kit.Button(c, uikit.ButtonOptions{Label: allLabel, Disabled: !ready || n.history.total == 0}).Clicked() {
				n.selectTagBatchMatches(batch)
			}
			kit.SettingsLabel(c, strings.ReplaceAll(n.text("tag_selected_count"), "{count}", strconv.FormatUint(count, 10)))
			if kit.Button(c, uikit.ButtonOptions{Label: n.text("common_delete"), Disabled: !ready || count == 0}).Clicked() {
				n.confirmTagBatch(batch)
			}
		})
		if batch.notice != "" {
			color := c.Theme().TextMuted
			if batch.failed {
				color = c.Theme().Danger
			}
			ui.Text(c, batch.notice).Wrap().TextColor(color)
		}
	})
}

func (n *nativeMain) confirmTagBatch(batch *nativeTagBatch) {
	if !n.tagBatchReady(batch) || n.reading || n.hasHistoryDialog() {
		return
	}
	count := batch.selection.count(n.history.total)
	if count == 0 {
		return
	}
	filter, state := n.history.filter, n.history.state
	message := n.text("tag_batch_delete_confirm")
	if batch.selection.all {
		message = n.text("tag_batch_delete_all_confirm")
	}
	message = strings.NewReplacer("{count}", strconv.FormatUint(count, 10), "{tag}", filter.tag).Replace(message)
	n.resetPreviewSelection()
	batch.pending = &nativeTagBatchConfirmation{
		open: true, generation: n.history.generation, revision: state.HistoryRevision,
		params: batch.selection.request(filter, state), message: message,
	}
}

func (n *nativeMain) tagBatchDialog(c *ui.Context, kit uikit.Kit, tags *nativeTagLibrary) {
	batch := &tags.batch
	dialog := batch.pending
	kit.Dialog(c, &dialog.open, n.text("tag_batch_delete_title"), n.text("common_close"), n.mutating, func() {
		kit.SettingsLabel(c, dialog.message).Wrap()
		ui.Row(c).Gap(6).Justify(ui.End).Children(func() {
			if kit.Button(c, uikit.ButtonOptions{Label: n.text("common_cancel"), Disabled: n.mutating}).Clicked() {
				dialog.open = false
			}
			if kit.Button(c, uikit.ButtonOptions{Label: n.text("common_delete"), Disabled: n.mutating}).Clicked() {
				n.deleteTagBatch(tags, dialog)
			}
		})
	})
	if !dialog.open {
		batch.pending = nil
	}
}

func (n *nativeMain) deleteTagBatch(tags *nativeTagLibrary, dialog *nativeTagBatchConfirmation) {
	batch := &tags.batch
	if n.tagLibrary != tags || batch.pending != dialog || n.mutating {
		return
	}
	if !n.tagBatchReady(batch) || dialog.generation != n.history.generation || dialog.revision != n.history.state.HistoryRevision {
		dialog.open = false
		batch.selection, batch.notice, batch.failed = nativeTagSelection{}, n.text("tag_selection_changed"), false
		return
	}
	n.mutating = true
	session, params := n.session, dialog.params
	go func() {
		ctx, cancel := context.WithTimeout(session, 10*time.Second)
		defer cancel()
		deleted, err := n.library.DeleteTagEntries(ctx, params)
		n.update(session, func() {
			n.mutating = false
			if n.tagLibrary != tags || batch.pending != dialog {
				return
			}
			batch.pending, batch.selection = nil, nativeTagSelection{}
			batch.failed = err != nil
			if err != nil {
				batch.notice = n.text("tag_action_failed")
			} else {
				batch.notice = strings.ReplaceAll(n.text("tag_batch_deleted"), "{count}", strconv.FormatUint(deleted, 10))
			}
			n.history.reset(n.history.state, n.history.filter)
			n.list.Reset()
			n.loadPage()
			n.tagCatalog.invalidate()
			n.loadTagCatalog()
		})
	}()
}
