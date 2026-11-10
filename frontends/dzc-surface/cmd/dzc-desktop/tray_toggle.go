package main

// UI-thread owned. A second tray click cancels a show awaiting Host recovery.
type trayToggleState struct {
	sequence uint64
	pending  bool
}

func (s *trayToggleState) toggle(shown bool) (uint64, bool) {
	s.sequence++
	if shown || s.pending {
		s.pending = false
		return 0, true
	}
	s.pending = true
	return s.sequence, false
}

func (s *trayToggleState) accept(sequence uint64) bool {
	if !s.pending || s.sequence != sequence {
		return false
	}
	s.pending = false
	return true
}

func (s *trayToggleState) cancel() { s.sequence++; s.pending = false }
