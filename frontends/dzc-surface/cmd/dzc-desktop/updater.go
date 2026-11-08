package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
)

const updateIndexBase = "https://github.com/qiyuanhuakai/dezirclip/releases/download/updater-channels/"

func updateRedirect(req *http.Request, previous []*http.Request) error {
	u := req.URL
	if len(previous) >= 5 || u.Scheme != "https" || u.User != nil || u.Port() != "" {
		return errors.New("update_untrusted_redirect")
	}
	switch u.Host {
	case "github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com", "github-releases.githubusercontent.com":
		return nil
	default:
		return errors.New("update_untrusted_redirect")
	}
}

func fetchUpdateIndex(ctx context.Context, transport *http.Client, endpoint string) (json.RawMessage, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, errors.New("update_invalid_endpoint")
	}
	req.Header.Set("Accept", "application/json")
	response, err := transport.Do(req)
	if err != nil {
		return nil, errors.New("update_check_failed")
	}
	defer response.Body.Close()
	if response.StatusCode == http.StatusNotFound {
		return nil, nil
	}
	if response.StatusCode != http.StatusOK {
		return nil, errors.New("update_server_failed")
	}
	const limit = 256 * 1024
	raw, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if err != nil {
		return nil, errors.New("update_check_failed")
	}
	if len(raw) > limit || !json.Valid(raw) {
		return nil, errors.New("update_invalid_manifest")
	}
	// Only an HTTP 404 denotes an unpublished channel.
	if bytes.TrimSpace(raw)[0] != '{' {
		return nil, errors.New("update_invalid_manifest")
	}
	return raw, nil
}

func (*DesktopShell) CheckForUpdate(ctx context.Context, channel string) (json.RawMessage, error) {
	return checkManualUpdate(ctx, channel, &http.Client{Timeout: 15 * time.Second, CheckRedirect: updateRedirect})
}

func checkManualUpdate(ctx context.Context, channel string, transport *http.Client) (json.RawMessage, error) {
	channels := []string{"stable"}
	switch channel {
	case "preview":
		channels = append(channels, "preview")
	case "stable":
	default:
		return nil, errors.New("update_invalid_endpoint")
	}
	indexes := make([]json.RawMessage, 0, len(channels))
	for _, name := range channels {
		manifest, err := fetchUpdateIndex(ctx, transport, updateIndexBase+name+".json")
		if err != nil {
			return nil, err
		}
		indexes = append(indexes, manifest)
	}
	return client.EvaluateUpdate(ctx, channel, indexes)
}
