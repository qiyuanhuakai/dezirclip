package main

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

// Both tag surfaces use the same catalog within a main-window presentation.
type nativeTagCatalog struct {
	counts                 map[string]uint64
	names                  []string
	invalidation, revision uint64
	fresh, loading, failed bool
}

func (catalog *nativeTagCatalog) invalidate() {
	catalog.invalidation++
	catalog.fresh, catalog.failed = false, false
}

func (n *nativeMain) loadTagCatalog() {
	catalog, state := &n.tagCatalog, n.history.state
	if n.session == nil || state.ServiceInstanceId == "" || catalog.loading || catalog.fresh {
		return
	}
	catalog.loading, catalog.failed = true, false
	session, stamp := n.session, catalog.invalidation
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		raw, err := n.library.TagCounts(ctx, state.PrivacyEpoch)
		var counts map[string]uint64
		if err == nil {
			err = json.Unmarshal(raw, &counts)
		}
		names := make([]string, 0, len(counts))
		for name := range counts {
			names = append(names, name)
		}
		slices.SortFunc(names, func(left, right string) int {
			if order := strings.Compare(strings.ToLower(left), strings.ToLower(right)); order != 0 {
				return order
			}
			return strings.Compare(left, right)
		})
		n.update(session, func() {
			catalog.loading = false
			if stamp != catalog.invalidation {
				return
			}
			catalog.fresh, catalog.failed = err == nil, err != nil
			if err == nil {
				catalog.counts, catalog.names = counts, names
				catalog.revision++
				if n.selectInitialTag() {
					n.editQuery(false)
				}
			}
		})
	}()
}

type nativeTagSuggestions struct {
	open, visible, focused, composing bool
	query, matchesQuery               string
	revision                          uint64
	matches                           []string
	active                            int
	list                              ui.ListState
}

func (picker *nativeTagSuggestions) dismiss() {
	picker.open, picker.visible = false, false
	picker.active = -1
}

func (picker *nativeTagSuggestions) refresh(query string, catalog *nativeTagCatalog) {
	picker.query = query
	if picker.matchesQuery == query && picker.revision == catalog.revision {
		return
	}
	changed := picker.matchesQuery != query
	var selected string
	if !changed && picker.active >= 0 && picker.active < len(picker.matches) {
		selected = picker.matches[picker.active]
	}
	picker.matchesQuery, picker.revision = query, catalog.revision
	needle := strings.TrimSpace(query)
	if tag, ok := searchTagName(query); ok {
		needle = tag
	}
	needle = strings.ToLower(needle)
	picker.matches = picker.matches[:0]
	for _, name := range catalog.names {
		if strings.Contains(strings.ToLower(name), needle) {
			picker.matches = append(picker.matches, name)
		}
	}
	picker.active = slices.Index(picker.matches, selected)
	if changed {
		picker.list.ScrollTo(0, ui.Start)
	}
}

func (picker *nativeTagSuggestions) move(key ui.Key) bool {
	last := len(picker.matches) - 1
	if last < 0 {
		return false
	}
	switch key {
	case ui.KeyDown:
		picker.active = min(picker.active+1, last)
	case ui.KeyUp:
		if picker.active < 0 {
			picker.active = last
		} else {
			picker.active = max(0, picker.active-1)
		}
	case ui.KeyHome:
		if picker.active < 0 {
			return false
		}
		picker.active = 0
	case ui.KeyEnd:
		if picker.active < 0 {
			return false
		}
		picker.active = last
	default:
		return false
	}
	picker.list.ScrollTo(picker.active, ui.Center)
	return true
}

func (n *nativeMain) tagSuggestionsView(c *ui.Context, input ui.Element) {
	picker, catalog := &n.tagSuggestions, &n.tagCatalog
	if n.session == nil || n.history.state.ServiceInstanceId == "" || n.failed || n.hasHistoryDialog() {
		picker.dismiss()
		picker.focused = false
		return
	}
	focused := input.Focused()
	picker.composing = input.Composing()
	if focused && !picker.composing && (!picker.focused || picker.query != n.query) {
		picker.open = true
	}
	picker.focused = focused
	if !picker.composing {
		picker.refresh(n.query, catalog)
	}
	picker.visible = false
	if !picker.open {
		return
	}
	if !catalog.fresh && !catalog.failed {
		n.loadTagCatalog()
	}
	_, explicitTag := searchTagName(picker.query)
	// A plain search can use Arrow keys for clipboard results once there are no
	// matching tags. A tag query retains the empty or loading message.
	if len(picker.matches) == 0 && !explicitTag && strings.TrimSpace(picker.query) != "" && !catalog.failed {
		return
	}
	picker.visible = true
	options := uikit.TagSuggestionsOptions{
		Label: n.text("tags"), Names: picker.matches, List: &picker.list, Active: &picker.active,
		Color: n.cardTagColor, OnSelect: n.selectSuggestedTag,
	}
	if catalog.failed {
		options.Retry = n.text("tag_load_failed") + " · " + n.text("common_retry")
		options.OnRetry = func() { n.loadTagCatalog(); n.focusSearch = true }
	}
	if len(picker.matches) == 0 {
		options.Status = n.text("tag_no_matching_tags")
		if catalog.loading && !catalog.failed {
			options.Status = n.text("common_loading")
		}
	} else if n.library != nil && n.cardTagColors == nil && !n.readingTagColors {
		n.loadCardTagColors()
	}
	panel := n.mainKit().TagSuggestions(c, input, &picker.open, options)
	if !focused && !panel.FocusWithin() && !panel.Hovered() {
		picker.dismiss()
	}
}

func (n *nativeMain) selectSuggestedTag(name string) {
	picker := &n.tagSuggestions
	picker.dismiss()
	n.query = "tag:" + name
	picker.query, picker.focused = n.query, true
	n.focusSearch = true
}

func (n *nativeMain) tagSuggestionKey(key ui.TextKey) bool {
	picker := &n.tagSuggestions
	if !picker.open || !picker.visible || picker.composing {
		return false
	}
	if key.Key == ui.KeyEscape {
		picker.dismiss()
		return true
	}
	picker.refresh(key.Text(), &n.tagCatalog)
	if key.Key == ui.KeyEnter {
		if picker.active >= 0 && picker.active < len(picker.matches) {
			n.selectSuggestedTag(picker.matches[picker.active])
			key.SetText(n.query)
			return true
		}
		// Don't paste a row behind the picker while the user is choosing a tag.
		return len(picker.matches) != 0
	}
	return picker.move(key.Key)
}
