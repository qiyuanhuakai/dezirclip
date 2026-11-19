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

func TestInlineSnapshotDoesNotRequestChunksAndRejectsInvalidData(t *testing.T) {
	state := protocol.RuntimeState{ServiceInstanceId: "inline-host", PrivacyEpoch: 2}
	client := &Client{hello: protocol.HelloResult{State: state}}
	text := "Synthetic UTF-8 示例"
	body, err := json.Marshal(protocol.ManualOcrText{Status: "done", Text: &text})
	if err != nil {
		t.Fatal(err)
	}
	snapshot := protocol.BodySnapshot{State: state, SnapshotId: strings.Repeat("0", 32),
		TotalBytes: uint32(len(body)), InlineJson: pointer(string(body))}
	var result protocol.ManualOcrText
	stats, err := client.ReadSnapshotMeasured(context.Background(), snapshot, "ManualOcrText", &result)
	if err != nil || stats.Chunks != 0 || result.Text == nil || *result.Text != text {
		t.Fatalf("inline snapshot did not decode directly: metrics %+v, error %v", stats, err)
	}
	for _, change := range []func(*protocol.BodySnapshot){
		func(candidate *protocol.BodySnapshot) { candidate.TotalBytes++ },
		func(candidate *protocol.BodySnapshot) { candidate.SnapshotId = strings.Repeat("a", 32) },
		func(candidate *protocol.BodySnapshot) { candidate.State.ServiceInstanceId = "old-host" },
		func(candidate *protocol.BodySnapshot) {
			candidate.InlineJson = pointer(strings.Repeat(" ", 32*1024+1))
			candidate.TotalBytes = uint32(len(*candidate.InlineJson))
		},
	} {
		candidate := snapshot
		change(&candidate)
		var result protocol.ManualOcrText
		if err := client.ReadSnapshot(context.Background(), candidate, "ManualOcrText", &result); err == nil || result.Text != nil {
			t.Fatal("invalid inline snapshot exposed content")
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
		{1, 2, false, "YWFh"}, {1, 2, false, "Yh=="},
	} {
		if decodeChunk(buffer, bad.offset, bad.next, bad.finished, bad.data) == nil {
			t.Fatal("bad screen chunk accepted")
		}
	}
	if buffer[0] != 0 || buffer[3] != 0 {
		t.Fatal("invalid chunk wrote outside its span")
	}
	if err := decodeChunk(buffer, 1, 3, false, "YQ\r\nA=\r\n"); err != nil || string(buffer) != "\x00a\x00\x00" {
		t.Fatal("legacy wrapped Base64 did not decode into its span", err)
	}
}
