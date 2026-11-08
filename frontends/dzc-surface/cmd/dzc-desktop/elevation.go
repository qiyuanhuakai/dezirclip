package main

import "context"

var elevatedInstance string

func (s *DesktopShell) RestartAsAdmin(ctx context.Context) error {
	// UAC is interactive. Do not time out and report cancellation while Windows
	// still holds an authorized elevation request that may later complete.
	return s.resident.send(ctx, residentRequest{Operation: "restart-admin"}, 0)
}
