package client

import (
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
	case "info", "add-to-path", "open-cli-guide", "open-theme-guide", "open-install-folder":
	default:
		return nil, ErrProtocol
	}
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	cmd, err := bundledCLI(ctx, "desktop-tools", action)
	if err != nil {
		return nil, err
	}
	raw, err := cmd.Output()
	if err != nil {
		var failed *exec.ExitError
		if errors.As(err, &failed) && len(failed.Stderr) > 0 {
			return nil, errors.New(strings.TrimSpace(string(failed.Stderr)))
		}
		return nil, err
	}
	if !json.Valid(raw) {
		return nil, ErrProtocol
	}
	return json.RawMessage(raw), nil
}
