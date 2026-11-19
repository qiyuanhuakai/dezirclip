package uikit

import "github.com/egoist/mygo/ui"

// HistoryListState keeps the visible position and the last pagination request.
// Give each independently filtered list its own state.
type HistoryListState struct {
	List          ui.ListState
	requestedRows int
	requested     bool
}

// Reset returns to the first row after a query change and allows pagination.
func (s *HistoryListState) Reset() {
	s.List.ScrollTo(0, ui.Start)
	s.requested = false
}

type HistoryListOptions struct {
	Count        int
	Label        string
	EmptyLabel   string
	LoadingLabel string
	MoreLabel    string
	Loading      bool
	HasMore      bool
	Compact      bool
	OnLoadMore   func()
	Row          func(ui.ListRow)
}

// HistoryList renders a viewport of variable-height history rows. The caller
// owns row identity, selection and reorder through the underlying ListState.
func (k Kit) HistoryList(c *ui.Context, state *HistoryListState, options HistoryListOptions) ui.Element {
	appearance := c.Theme()
	listTheme := *appearance
	listTheme.Accent, listTheme.SurfaceHover = ui.Color{}, ui.Color{}
	listTheme.AccentText = appearance.Text
	c.SetTheme(&listTheme)
	gap := float32(4)
	if options.Compact {
		gap = 2
	}
	list := ui.List(c, &state.List, options.Count).Grow(1).FillWidth().Gap(gap).Label(options.Label)
	list.Rows(func(row ui.ListRow) {
		row.Context.SetTheme(appearance)
		options.Row(row)
		row.Context.SetTheme(&listTheme)
	})
	c.SetTheme(appearance)
	if options.Count == 0 {
		message := options.EmptyLabel
		if options.Loading {
			message = options.LoadingLabel
		}
		list.Children(func() {
			ui.Text(c, message).Padding(16).TextColor(appearance.TextMuted)
		})
	}
	if !state.List.AtEnd() {
		return list
	}
	if options.HasMore && !options.Loading && options.OnLoadMore != nil &&
		(!state.requested || state.requestedRows != options.Count) {
		state.requested, state.requestedRows = true, options.Count
		options.OnLoadMore()
	}
	if options.Count > 0 && (options.Loading || options.HasMore) {
		label := options.MoreLabel
		if options.Loading {
			label = options.LoadingLabel
		}
		ui.Row(c).FillWidth().Justify(ui.Center).Padding(20).Children(func() {
			ui.Text(c, label).FontSize(12).TextColor(appearance.TextMuted).Opacity(0.6)
		})
	}
	return list
}
