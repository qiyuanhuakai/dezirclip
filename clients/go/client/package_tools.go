package client

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os/exec"
	"strings"
	"time"
)

// PackageTool addresses only this distribution's CLI, even if another is on PATH.
func PackageTool(ctx context.Context, action string) (json.RawMessage, error) {
	switch action {
	case "info", "add-to-path", "open-cli-guide", "open-theme-guide", "open-install-folder",
		"login-status", "enable-login", "disable-login", "installed-apps", "default-apps", "update-support":
	default:
		return nil, ErrProtocol
	}
	return packageTool(ctx, action, nil)
}

// EvaluateUpdate passes only already downloaded channel metadata to the offline Rust policy.
func EvaluateUpdate(ctx context.Context, channel string, indexes []json.RawMessage) (json.RawMessage, error) {
	var input bytes.Buffer
	encoder := json.NewEncoder(&input)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(struct {
		Channel string            `json:"channel"`
		Indexes []json.RawMessage `json:"indexes"`
	}{channel, indexes}); err != nil {
		return nil, errors.New("update_invalid_manifest")
	}
	return packageTool(ctx, "evaluate-update", input.Bytes())
}

func packageTool(ctx context.Context, action string, input []byte) (json.RawMessage, error) {
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	cmd, err := bundledCLI(ctx, "desktop-tools", action)
	if err != nil {
		return nil, err
	}
	cmd.Stdin = bytes.NewReader(input)
	raw, err := cmd.Output()
	if err != nil {
		var failed *exec.ExitError
		if errors.As(err, &failed) && len(failed.Stderr) > 0 {
			return nil, errors.New(strings.TrimPrefix(strings.TrimSpace(string(failed.Stderr)), "error: "))
		}
		return nil, err
	}
	if !json.Valid(raw) {
		return nil, ErrProtocol
	}
	return json.RawMessage(raw), nil
}
