package quickpaste

import "github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/model"

// Queue outside the render pass: beginning a query revokes the frame's rows.
// One pending callback and the model's loading flag bound automatic requests.
type pagePrefetch struct{ queued bool }

func (p *pagePrefetch) nearEnd(m *model.Model, last int, post func(func()), read func(uint32)) {
	if p.queued || !m.CanSelect() || !m.HasMore || m.Error != "" || last < 0 || last < len(m.Entries)-3 {
		return
	}
	ticket := model.Ticket{Session: m.Session, Display: m.Display, Query: m.RawQuery,
		Sequence: m.Sequence, State: m.State, Offset: m.NextOffset}
	p.queued = true
	post(func() {
		p.queued = false
		if m.Current(ticket) && m.CanSelect() && m.HasMore && m.Error == "" && m.NextOffset == ticket.Offset {
			read(ticket.Offset)
		}
	})
}
