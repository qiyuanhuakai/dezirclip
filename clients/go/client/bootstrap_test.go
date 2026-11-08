package client

import (
	"errors"
	"io"
	"strings"
	"testing"
)

func TestDesktopStderrDrainsOverflowAndPreservesCause(t *testing.T) {
	var stderr desktopStderr
	input := strings.Repeat("x", 8192)
	if n, err := io.Copy(&stderr, strings.NewReader(input)); err != nil || n != int64(len(input)) {
		t.Fatalf("stderr was not drained: %d, %v", n, err)
	}
	if stderr.n != len(stderr.data) {
		t.Fatalf("retained %d bytes", stderr.n)
	}
	cause := errors.New("exit status 1")
	err := stderr.exitError(cause)
	if !errors.Is(err, cause) || err.Error() != "desktop-endpoint: exit status 1: "+input[:1024] {
		t.Fatalf("unexpected diagnostic: %v", err)
	}
	var empty desktopStderr
	if err := empty.exitError(cause); !errors.Is(err, cause) || err.Error() != "desktop-endpoint: exit status 1" {
		t.Fatalf("unexpected empty diagnostic: %v", err)
	}
}
