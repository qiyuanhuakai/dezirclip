package application

import (
	"context"
	"encoding/base64"
	"errors"
	"io"
	"os"
	"strings"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func EmojiFavorites(ctx context.Context, owner *client.Client, epoch uint64, path *string) (protocol.EmojiFavoritesResult, error) {
	var result protocol.EmojiFavoritesResult
	params := protocol.EmojiFavoritesParams{ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path}
	err := owner.DesktopCall(ctx, protocol.MethodDesktopEmojiFavorites, params, "EmojiFavoritesResult", &result)
	return result, err
}

func MutateFavorite(ctx context.Context, owner *client.Client, epoch uint64, action, path string) (string, error) {
	var result protocol.EmojiFavoritesResult
	params := protocol.EmojiFavoritesParams{ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: &path, Mutation: &action}
	err := owner.DesktopCall(ctx, protocol.MethodDesktopEmojiFavorites, params, "EmojiFavoritesResult", &result)
	if err != nil || action == "remove" {
		return "", err
	}
	if len(result.Paths) != 1 {
		return "", client.ErrProtocol
	}
	return result.Paths[0], nil
}

// Browser drops may supply bytes without a filesystem path. Only the temporary path crosses IPC.
func ImportFavoriteData(ctx context.Context, owner *client.Client, epoch uint64, dataURL string) (string, error) {
	header, payload, ok := strings.Cut(dataURL, ",")
	const limit = 16 << 20
	if !ok || !strings.HasPrefix(header, "data:image/") || !strings.HasSuffix(header, ";base64") || len(payload) > base64.StdEncoding.EncodedLen(limit) {
		return "", errors.New("invalid favorite image")
	}
	staging, err := os.CreateTemp("", "dzc-emoji-*")
	if err != nil {
		return "", err
	}
	defer os.Remove(staging.Name())
	n, copyErr := io.Copy(staging, io.LimitReader(base64.NewDecoder(base64.StdEncoding, strings.NewReader(payload)), limit+1))
	closeErr := staging.Close()
	if copyErr != nil {
		return "", copyErr
	}
	if closeErr != nil {
		return "", closeErr
	}
	if n == 0 || n > limit {
		return "", errors.New("invalid favorite image size")
	}
	return MutateFavorite(ctx, owner, epoch, "import", staging.Name())
}
