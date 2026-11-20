package main

import (
	"bytes"
	"encoding/base64"
	"io"
	"slices"
	"strings"
	"testing"
)

func TestUpdateDownloadReportsWrittenBytesAndRejectsTruncation(t *testing.T) {
	for _, total := range []int64{6, -1} {
		var written bytes.Buffer
		var updates [][2]int64
		err := copyUpdateBody(&written, strings.NewReader("update"), total, func(written, total int64) {
			updates = append(updates, [2]int64{written, total})
		})
		if err != nil || written.String() != "update" || !slices.Equal(updates, [][2]int64{{0, total}, {6, total}}) {
			t.Fatalf("download length %d wrote %q and reported %v: %v", total, written.String(), updates, err)
		}
	}
	if err := copyUpdateBody(io.Discard, strings.NewReader("update"), 7, nil); err != errUpdateVerification {
		t.Fatalf("truncated update returned %v", err)
	}
}

func TestVerifyMyGoUpdateUsesAuthenticatedVersionFilenameAndBytes(t *testing.T) {
	// Public Tauri updater 2.14.0 test vector.
	key := "untrusted comment: minisign public key: A49EF4746FA6E6D7\nRWTX5qZvdPSepDGWg2bz8n6ArQnBMwgQK1geVosorcbLzcunD+KJ+cYe"
	signature := "untrusted comment: signature from tauri secret key\n" +
		"RUTX5qZvdPSepPF2zsjvP/L/TQtBLRnWRvaq+4omK1UEugtQp/USrGiI1v762sDy3id3aor7bRC9ETyeot6lMHHrWoZ5hjUM1Qc=\n" +
		"trusted comment: timestamp:1791649051\tfile:payload\tversion:2.0.0\n" +
		"DDq2CLOEl5cQmC2eJRHlnZPVViOZsu1CjpPKQoei4A/XBfkDqGO0B+QMrcrHgEQ5qV3mne75tdFWhJpqUXh9AA=="
	encode := func(s string) string { return base64.StdEncoding.EncodeToString([]byte(s)) }
	for _, test := range []struct {
		name, bytes, sig, version, filename string
		want                                bool
	}{
		{"signed", "test", signature, "2.0.0", "payload", true},
		{"changed bytes", "tEst", signature, "2.0.0", "payload", false},
		{"other version", "test", signature, "2.0.1", "payload", false},
		{"other package", "test", signature, "2.0.0", "other", false},
		{"changed comment", "test", strings.Replace(signature, "version:2.0.0", "version:2.0.1", 1), "2.0.1", "payload", false},
	} {
		t.Run(test.name, func(t *testing.T) {
			err := verifyMyGoUpdate(strings.NewReader(test.bytes), encode(key), encode(test.sig), test.version, test.filename)
			if (err == nil) != test.want {
				t.Fatalf("verifyMyGoUpdate returned %v", err)
			}
		})
	}
}
