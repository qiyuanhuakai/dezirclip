package client

import (
	"bytes"
	"context"
	"encoding/base64"
	"image/png"
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
