package model

// ViewRow contains only the values rendered by a single frame. It never retains
// an EntrySummary, its preview pointer, or its tags.
type ViewRow struct {
	ID        int64
	Text      string
	SourceApp string
}

// View keeps one frame's immutable row values until the frame is revoked. A
// toolkit may retain already-built text until it replaces its own render tree;
// revocation clears our values and prevents old callbacks from rendering more.
type View struct {
	ticket  Ticket
	rows    []ViewRow
	ready   bool
	revoked bool
}

func (m *Model) CaptureView(hidden string) *View {
	v := &View{
		ticket: Ticket{Session: m.Session, Display: m.Display, Query: m.RawQuery,
			Sequence: m.Sequence, State: m.State},
		ready: m.CanSelect(),
		rows:  make([]ViewRow, len(m.Entries)),
	}
	for i, entry := range m.Entries {
		v.rows[i] = ViewRow{ID: entry.Id, Text: DisplayText(entry, hidden), SourceApp: entry.SourceApp}
	}
	return v
}

func (v *View) Revoke() {
	if v == nil {
		return
	}
	clear(v.rows)
	v.rows = nil
	v.ticket = Ticket{}
	v.ready, v.revoked = false, true
}

func (v *View) Current(m *Model) bool {
	if v == nil || v.revoked {
		return false
	}
	if m == nil || !m.Current(v.ticket) {
		v.Revoke()
		return false
	}
	return true
}

func (v *View) Len(m *Model) int {
	if !v.Current(m) {
		return 0
	}
	return len(v.rows)
}

func (v *View) Row(m *Model, index int) (ViewRow, bool) {
	if !v.Current(m) || index < 0 || index >= len(v.rows) {
		return ViewRow{}, false
	}
	return v.rows[index], true
}

func (v *View) SelectedID(m *Model) (int64, bool) {
	if !v.Current(m) || !v.ready || !m.CanSelect() {
		return 0, false
	}
	row, ok := v.Row(m, m.Selected)
	if !ok || m.Entries[m.Selected].Id != row.ID {
		return 0, false
	}
	return row.ID, true
}
