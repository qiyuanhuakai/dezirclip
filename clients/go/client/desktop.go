package client

import (
	"bytes"
	"context"
	"encoding/base64"
	"image/png"
	"slices"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// ReadSnapshot uses the connection that received the descriptor. No partial
// result is decoded if the Host replaces the snapshot or revokes its epoch.
func (client *Client) ReadSnapshot(ctx context.Context, snapshot protocol.BodySnapshot, schema string, result any) error {
	if snapshot.State.ServiceInstanceId != client.hello.State.ServiceInstanceId || snapshot.TotalBytes == 0 || snapshot.TotalBytes > protocol.MaxSnapshotBytes {
		return ErrProtocol
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	raw := make([]byte, snapshot.TotalBytes)
	defer clear(raw)
	for offset := uint32(0); offset < snapshot.TotalBytes; {
		request := protocol.SnapshotReadParams{SnapshotId: snapshot.SnapshotId, Offset: offset,
			ExpectedEpoch: snapshot.State.PrivacyEpoch, ServiceInstanceId: snapshot.State.ServiceInstanceId}
		var chunk protocol.SnapshotChunk
		if err := client.Call(ctx, protocol.MethodDesktopSnapshotRead, request, "SnapshotChunk", &chunk); err != nil {
			return err
		}
		if chunk.State.ServiceInstanceId != request.ServiceInstanceId || chunk.State.PrivacyEpoch != request.ExpectedEpoch || chunk.Offset != offset {
			return ErrProtocol
		}
		if err := decodeChunk(raw, offset, chunk.NextOffset, chunk.Finished, chunk.DataBase64); err != nil {
			return err
		}
		offset = chunk.NextOffset
	}
	return protocol.DecodeSnapshot(schema, raw, result)
}

// ReadScreenPNG returns a complete, dimension-checked PNG from begin or crop.
func (client *Client) ReadScreenPNG(ctx context.Context, state protocol.RuntimeState, image protocol.ScreenImage) ([]byte, error) {
	if state.ServiceInstanceId != client.hello.State.ServiceInstanceId || image.PngBytes == 0 || image.PngBytes > 160*1024*1024 {
		return nil, ErrProtocol
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	raw := make([]byte, image.PngBytes)
	for offset := uint32(0); offset < image.PngBytes; {
		request := protocol.ScreenReadParams{Token: image.Token, Offset: offset, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch}
		var chunk protocol.ScreenChunk
		if err := client.Call(ctx, protocol.MethodDesktopScreenRead, request, "ScreenChunk", &chunk); err != nil {
			clear(raw)
			return nil, err
		}
		if err := decodeChunk(raw, offset, chunk.NextOffset, chunk.Finished, chunk.DataBase64); err != nil {
			clear(raw)
			return nil, err
		}
		offset = chunk.NextOffset
	}
	info, err := png.DecodeConfig(bytes.NewReader(raw))
	if err != nil || uint32(info.Width) != image.Width || uint32(info.Height) != image.Height {
		clear(raw)
		return nil, ErrProtocol
	}
	return raw, nil
}

func decodeChunk(buffer []byte, offset, next uint32, finished bool, encoded string) error {
	if next <= offset || uint64(next) > uint64(len(buffer)) || finished != (int(next) == len(buffer)) {
		return ErrProtocol
	}
	decoded, err := base64.StdEncoding.Strict().DecodeString(encoded)
	defer clear(decoded)
	if err != nil || len(decoded) != int(next-offset) {
		return ErrProtocol
	}
	copy(buffer[offset:next], decoded)
	return nil
}

// BeginQuickPaste captures the system target before showing any desktop window.
func (client *Client) BeginQuickPaste(ctx context.Context, state protocol.RuntimeState) (protocol.QuickTicket, error) {
	var ticket protocol.QuickTicket
	err := client.Call(ctx, protocol.MethodDesktopQuickBegin, protocol.LibraryStateParams{
		ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
	}, "QuickTicket", &ticket)
	return ticket, err
}

// PasteWindowSelection lets the authenticated Host restore the original editor
// while this window still holds Windows foreground activation rights.
func (client *Client) PasteWindowSelection(ctx context.Context, params protocol.DesktopCopyParams) error {
	if !params.Paste || params.ServiceInstanceId != client.hello.State.ServiceInstanceId {
		return ErrProtocol
	}
	if err := grantFocusRestore(client.conn); err != nil {
		return err
	}
	var result protocol.OperationResult
	err := client.Call(ctx, protocol.MethodDesktopWindowPaste, params, "OperationResult", &result)
	if err == nil && !result.Performed {
		return ErrProtocol
	}
	return err
}

func (client *Client) checkFocusTicket(ticket protocol.QuickTicket, state protocol.RuntimeState) error {
	if !slices.Contains(client.hello.Permissions, protocol.PermissionDesktopHistory) ||
		ticket.State.ServiceInstanceId != client.hello.State.ServiceInstanceId ||
		state.ServiceInstanceId != ticket.State.ServiceInstanceId || state.PrivacyEpoch != ticket.State.PrivacyEpoch {
		return ErrProtocol
	}
	return nil
}

// PasteQuickSelection consumes the ticket and uses the revision that supplied
// the selected row. The caller hides the panel after a successful result.
func (client *Client) PasteQuickSelection(ctx context.Context, ticket protocol.QuickTicket, state protocol.RuntimeState, id int64) error {
	if err := client.checkFocusTicket(ticket, state); err != nil {
		return err
	}
	if err := grantFocusRestore(client.conn); err != nil {
		return err
	}
	var result protocol.OperationResult
	err := client.Call(ctx, protocol.MethodDesktopQuickPaste, protocol.QuickPasteParams{ServiceInstanceId: state.ServiceInstanceId,
		ExpectedEpoch: state.PrivacyEpoch, ExpectedHistoryRevision: state.HistoryRevision, Token: ticket.Token, Id: id}, "OperationResult", &result)
	if err == nil && !result.Performed {
		return ErrProtocol
	}
	return err
}

func (client *Client) DismissQuickPaste(ctx context.Context, ticket protocol.QuickTicket, restore bool) error {
	if err := client.checkFocusTicket(ticket, ticket.State); err != nil {
		return err
	}
	if restore {
		if err := grantFocusRestore(client.conn); err != nil {
			return err
		}
	}
	var result protocol.OperationResult
	return client.Call(ctx, protocol.MethodDesktopQuickDismiss, protocol.QuickDismissParams{ServiceInstanceId: ticket.State.ServiceInstanceId,
		ExpectedEpoch: ticket.State.PrivacyEpoch, Token: ticket.Token, Restore: restore}, "OperationResult", &result)
}
