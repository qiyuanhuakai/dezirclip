// Package model fences ephemeral Surface metadata without native UI or IO.
package model

import (
	"math"
	"strings"
	"unicode"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

const PageSize = 50
const MaxVisibleEntries = 1000

func NativeLocale(locale string) string {
	if locale == "tw" {
		return "zh-TW"
	}
	return locale
}

// Ticket binds a result to the view that requested it. No response changes its
// original Host, epoch or query in order to become current again.
type Ticket struct {
	Session, Display, Query string
	Sequence                uint64
	State                   protocol.RuntimeState
	Offset                  uint32
}

type Model struct {
	Session, Display            string
	State                       protocol.RuntimeState
	Visible, Loading, Composing bool
	RawQuery, AppliedQuery      string
	Sequence                    uint64
	Entries                     []protocol.EntrySummary
	Selected                    int
	HasMore                     bool
	NextOffset                  uint32
	PendingAction               string
	Error                       string
	ShowNotice                  bool
}

func SameIdentity(a, b protocol.RuntimeState) bool {
	return a.ServiceInstanceId == b.ServiceInstanceId && a.PrivacyEpoch == b.PrivacyEpoch && a.HistoryRevision == b.HistoryRevision
}

func (m *Model) Show(session, display string, state protocol.RuntimeState) {
	m.Clear()
	m.Session, m.Display, m.State, m.Visible = session, display, state, true
	m.Loading = true
}

func (m *Model) Clear() {
	m.Sequence++
	m.Display, m.RawQuery, m.AppliedQuery, m.PendingAction, m.Error = "", "", "", "", ""
	m.State = protocol.RuntimeState{}
	m.clearResults()
	m.Visible, m.Loading, m.Composing, m.HasMore = false, false, false, false
	m.ShowNotice = false
}

func (m *Model) clearResults() {
	m.Entries, m.HasMore, m.NextOffset = nil, false, 0
	m.Selected, m.AppliedQuery = 0, ""
}

func (m *Model) Edit(query string, composing bool) bool {
	if m.RawQuery == query && m.Composing == composing {
		return false
	}
	m.Sequence++
	m.RawQuery, m.Composing = query, composing
	m.clearResults()
	m.Error = ""
	m.Loading = true
	return true
}

func (m *Model) Begin(offset uint32) Ticket {
	m.Sequence++
	if offset == 0 {
		m.clearResults()
	}
	m.Loading, m.Error = true, ""
	return Ticket{m.Session, m.Display, m.RawQuery, m.Sequence, m.State, offset}
}

func (m *Model) Fail(t Ticket, message string) bool {
	if !m.Current(t) {
		return false
	}
	m.Sequence++
	m.clearResults()
	m.Loading, m.Error = false, message
	return true
}

func (m *Model) Current(t Ticket) bool {
	return m.Visible && m.Session == t.Session && m.Display == t.Display && m.Sequence == t.Sequence && m.RawQuery == t.Query && SameIdentity(m.State, t.State)
}

func (m *Model) Apply(t Ticket, page protocol.SurfacePage) bool {
	if !m.Current(t) || !SameIdentity(t.State, page.State) || page.State.EventSequence < t.State.EventSequence || len(page.Entries) > PageSize {
		return false
	}
	if t.Offset != uint32(len(m.Entries)) {
		return false
	}
	if page.HasMore && (page.NextOffset == nil || len(page.Entries) == 0 || *page.NextOffset != t.Offset+uint32(len(page.Entries))) {
		return false
	}
	if !page.HasMore && page.NextOffset != nil {
		return false
	}
	seen := make(map[int64]bool, len(m.Entries)+len(page.Entries))
	for _, entry := range m.Entries {
		seen[entry.Id] = true
	}
	for _, entry := range page.Entries {
		if seen[entry.Id] {
			return false
		}
		seen[entry.Id] = true
	}
	if len(m.Entries)+len(page.Entries) > MaxVisibleEntries {
		return false
	}
	m.Entries = append(m.Entries, page.Entries...)
	m.State, m.AppliedQuery, m.Loading = page.State, t.Query, false
	m.HasMore = page.HasMore && len(m.Entries) < MaxVisibleEntries
	m.NextOffset = uint32(len(m.Entries))
	return true
}

func (m *Model) CanSelect() bool {
	return m.Visible && !m.Loading && !m.Composing && m.RawQuery == m.AppliedQuery && m.PendingAction == "" && m.Selected >= 0 && m.Selected < len(m.Entries)
}

func (m *Model) Move(delta int) {
	if m.CanSelect() {
		m.Selected = max(0, min(len(m.Entries)-1, m.Selected+delta))
	}
}

// DisplayText is metadata-only and bounds visual work. Protected summaries are
// never replaced with an automatic request for their body or a resource.
func DisplayText(entry protocol.EntrySummary, hidden string) string {
	if entry.PrivacyVisibility != "ordinary" {
		return hidden
	}
	if entry.Preview == nil || strings.TrimSpace(*entry.Preview) == "" {
		return "[" + entry.Kind + "]"
	}
	var out strings.Builder
	count := 0
	for _, r := range *entry.Preview {
		if count == 240 {
			out.WriteRune('…')
			break
		}
		if unicode.IsControl(r) {
			r = ' '
		}
		out.WriteRune(r)
		count++
	}
	return out.String()
}

type Bounds struct{ X, Y, Width, Height int }

// MyGo's public window API uses DIPs; the parent sends physical coordinates.
func LogicalBounds(rect protocol.SurfacePhysicalRect, scale float64) (Bounds, bool) {
	if math.IsNaN(scale) || math.IsInf(scale, 0) || scale < 0.25 || scale > 16 || rect.X < math.MinInt32 || rect.X > math.MaxInt32 || rect.Y < math.MinInt32 || rect.Y > math.MaxInt32 || rect.Width == 0 || rect.Height == 0 || rect.Width > 16384 || rect.Height > 16384 {
		return Bounds{}, false
	}
	values := []float64{float64(rect.X) / scale, float64(rect.Y) / scale, float64(rect.Width) / scale, float64(rect.Height) / scale}
	for _, value := range values {
		if value < math.MinInt32 || value > math.MaxInt32 {
			return Bounds{}, false
		}
	}
	b := Bounds{int(math.Round(values[0])), int(math.Round(values[1])), int(math.Round(values[2])), int(math.Round(values[3]))}
	return b, b.Width > 0 && b.Height > 0
}
