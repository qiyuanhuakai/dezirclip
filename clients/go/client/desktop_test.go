package client

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net"
	"strings"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestDesktopSnapshotReassemblesAcrossFrameLimitAndRejectsChangedEpoch(t *testing.T) {
	text := strings.Repeat("x", protocol.MaxFrameBytes+1)
	raw, err := json.Marshal(protocol.ManualOcrText{Status: "done", Text: &text})
	if err != nil {
		t.Fatal(err)
	}
	var tooLarge protocol.ManualOcrText
	if protocol.Decode("ManualOcrText", raw, &tooLarge) == nil {
		t.Fatal("single-frame limit lost")
	}
	for _, changed := range []bool{false, true} {
		local, peer := net.Pipe()
		state := protocol.RuntimeState{ServiceInstanceId: "test-host", PrivacyEpoch: 2}
		client := &Client{conn: local, hello: protocol.HelloResult{State: state}}
		done := make(chan error, 1)
		go func() {
			defer peer.Close()
			for offset := 0; offset < len(raw); {
				requestBytes, err := readFrame(peer)
				if err != nil {
					done <- err
					return
				}
				var request protocol.Request
				if err = json.Unmarshal(requestBytes, &request); err != nil {
					done <- err
					return
				}
				var params protocol.SnapshotReadParams
				if err = json.Unmarshal(request.Params, &params); err != nil {
					done <- err
					return
				}
				if request.Method != protocol.MethodDesktopSnapshotRead || params.Offset != uint32(offset) {
					done <- ErrProtocol
					return
				}
				end := min(len(raw), offset+192*1024)
				chunkState := state
				if changed && offset > 0 {
					chunkState.PrivacyEpoch++
				}
				chunk := protocol.SnapshotChunk{State: chunkState, Offset: uint32(offset), NextOffset: uint32(end),
					Finished: end == len(raw), DataBase64: base64.StdEncoding.EncodeToString(raw[offset:end])}
				payload, err := json.Marshal(chunk)
				if err != nil {
					done <- err
					return
				}
				response, err := json.Marshal(protocol.Response{RequestId: request.RequestId, Result: pointer(json.RawMessage(payload))})
				if err != nil {
					done <- err
					return
				}
				if err = writeFrame(peer, response); err != nil {
					done <- err
					return
				}
				if changed && offset > 0 {
					break
				}
				offset = end
			}
			done <- nil
		}()
		snapshot := protocol.BodySnapshot{SnapshotId: strings.Repeat("a", 32), TotalBytes: uint32(len(raw)), State: state}
		var result protocol.ManualOcrText
		err := client.ReadSnapshot(context.Background(), snapshot, "ManualOcrText", &result)
		client.Close()
		if fixtureError := <-done; fixtureError != nil {
			t.Fatal(fixtureError, err)
		}
		if changed {
			if err != ErrProtocol || result.Text != nil {
				t.Fatal("mixed snapshot returned content")
			}
		} else if err != nil || result.Text == nil || *result.Text != text {
			t.Fatal("snapshot content mismatch", err)
		}
	}
}

func TestScreenChunkRejectsBadBoundaries(t *testing.T) {
	buffer := make([]byte, 4)
	for _, bad := range []struct {
		offset, next uint32
		finished     bool
		data         string
	}{
		{0, 0, false, ""}, {0, 5, true, "YQ=="}, {0, 1, true, "YQ=="}, {0, 1, false, "?"}, {0, 2, false, "YQ=="},
	} {
		if decodeChunk(buffer, bad.offset, bad.next, bad.finished, bad.data) == nil {
			t.Fatal("bad screen chunk accepted")
		}
	}
}
