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
	return client.readSnapshot(ctx, snapshot, schema, result, nil)
}

// SnapshotReadMetrics summarizes the chunk exchanges and final schema decode.
type SnapshotReadMetrics struct {
	Chunks   int
	Transfer time.Duration
	Decode   time.Duration
}

func (client *Client) ReadSnapshotMeasured(ctx context.Context, snapshot protocol.BodySnapshot, schema string, result any) (SnapshotReadMetrics, error) {
	var metrics SnapshotReadMetrics
	err := client.readSnapshot(ctx, snapshot, schema, result, &metrics)
	return metrics, err
}

func (client *Client) readSnapshot(ctx context.Context, snapshot protocol.BodySnapshot, schema string, result any, metrics *SnapshotReadMetrics) error {
	if snapshot.State.ServiceInstanceId != client.hello.State.ServiceInstanceId || snapshot.TotalBytes == 0 || snapshot.TotalBytes > protocol.MaxSnapshotBytes {
		return ErrProtocol
	}
	if snapshot.InlineJson != nil {
		const maxInlineBytes = 32 * 1024
		const inlineSnapshotID = "00000000000000000000000000000000"
		body := *snapshot.InlineJson
		if snapshot.SnapshotId != inlineSnapshotID || len(body) > maxInlineBytes || len(body) != int(snapshot.TotalBytes) {
			return ErrProtocol
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		started := time.Now()
		data := []byte(body)
		defer clear(data)
		err := protocol.DecodeSnapshot(schema, data, result)
		if metrics != nil {
			metrics.Decode = time.Since(started)
		}
		return err
	}
	var transferStarted time.Time
	if metrics != nil {
		transferStarted = time.Now()
		defer func() {
			if metrics.Transfer == 0 {
				metrics.Transfer = time.Since(transferStarted)
			}
		}()
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	raw := make([]byte, snapshot.TotalBytes)
	defer clear(raw)
	for offset := uint32(0); offset < snapshot.TotalBytes; {
		request := protocol.SnapshotReadParams{SnapshotId: snapshot.SnapshotId, Offset: offset,
			ExpectedEpoch: snapshot.State.PrivacyEpoch, ServiceInstanceId: snapshot.State.ServiceInstanceId}
		if metrics != nil {
			metrics.Chunks++
		}
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
	if metrics == nil {
		return protocol.DecodeSnapshot(schema, raw, result)
	}
	decodeStarted := time.Now()
	metrics.Transfer = decodeStarted.Sub(transferStarted)
	err := protocol.DecodeSnapshot(schema, raw, result)
	metrics.Decode = time.Since(decodeStarted)
	return err
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
	// Limit decoding to the current chunk so a bad reply cannot write into a neighbour.
	target := buffer[offset:next:next]
	decoded, err := base64.StdEncoding.Strict().AppendDecode(target[:0], []byte(encoded))
	if err != nil || len(decoded) != len(target) {
		clear(decoded)
		return ErrProtocol
	}
	if &decoded[0] != &target[0] {
		// Unusual line breaks can make AppendDecode allocate despite a correct decoded length.
		copy(target, decoded)
		clear(decoded)
	}
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
	return client.callWindowPaste(ctx, protocol.MethodDesktopWindowPaste, params)
}

func (client *Client) PasteWindowEmoji(ctx context.Context, params protocol.WindowEmojiParams) error {
	if !params.Paste || params.ServiceInstanceId != client.hello.State.ServiceInstanceId {
		return ErrProtocol
	}
	return client.callWindowPaste(ctx, protocol.MethodDesktopWindowEmoji, params)
}

func (client *Client) callWindowPaste(ctx context.Context, method protocol.Method, params any) error {
	if err := grantFocusRestore(client.conn); err != nil {
		return err
	}
	var result protocol.OperationResult
	err := client.Call(ctx, method, params, "OperationResult", &result)
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
