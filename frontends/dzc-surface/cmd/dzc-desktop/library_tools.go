package main

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

func (l *Library) PreviewBackup(ctx context.Context, epoch uint64, path string) ([]protocol.BackupPreviewRow, error) {
	var result protocol.BackupPreview
	params := protocol.BackupPreviewParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopBackupPreview, params, "BackupPreview", &result)
	return result.Entries, err
}

func (l *Library) ImportBackup(ctx context.Context, epoch uint64, path, mode string, passphrase *string) (protocol.BackupImportResult, error) {
	var result protocol.BackupImportResult
	params := protocol.BackupImportParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path, Mode: mode, Passphrase: passphrase}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopBackupImport, params, "BackupImportResult", &result)
	return result, err
}

func (l *Library) ExportBackup(ctx context.Context, epoch uint64, path, format string, passphrase *string) (protocol.BackupExportResult, error) {
	var result protocol.BackupExportResult
	params := protocol.BackupExportParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path, Format: format, Passphrase: passphrase}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopBackupExport, params, "BackupExportResult", &result)
	return result, err
}

func (l *Library) ToolCatalog(ctx context.Context) (protocol.ToolCatalog, error) {
	var catalog protocol.ToolCatalog
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopToolsCatalog, protocol.ToolCatalogParams{}, "ToolCatalog", &catalog)
	return catalog, err
}

func (l *Library) SystemFonts(ctx context.Context) ([]protocol.DesktopFont, error) {
	var list protocol.DesktopFontList
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopSettingsFonts, protocol.ToolCatalogParams{}, "DesktopFontList", &list)
	return list.Fonts, err
}

func (l *Library) EmojiFavorites(ctx context.Context, epoch uint64, path *string) (protocol.EmojiFavoritesResult, error) {
	var result protocol.EmojiFavoritesResult
	params := protocol.EmojiFavoritesParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopEmojiFavorites, params, "EmojiFavoritesResult", &result)
	return result, err
}

func (l *Library) MutateFavorite(ctx context.Context, epoch uint64, action, path string) (string, error) {
	var result protocol.EmojiFavoritesResult
	params := protocol.EmojiFavoritesParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: &path, Mutation: &action}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopEmojiFavorites, params, "EmojiFavoritesResult", &result)
	if err != nil || action == "remove" {
		return "", err
	}
	if len(result.Paths) != 1 {
		return "", client.ErrProtocol
	}
	return result.Paths[0], nil
}

// Browser drops may supply bytes without a filesystem path. Only the temporary path crosses IPC.
func (l *Library) ImportFavoriteData(ctx context.Context, epoch uint64, dataURL string) (string, error) {
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
	return l.MutateFavorite(ctx, epoch, "import", staging.Name())
}

func (l *Library) Classify(ctx context.Context, text string) ([]string, error) {
	var result protocol.ToolClassification
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopToolsClassify, protocol.ToolClassifyParams{Text: text}, "ToolClassification", &result)
	return result.Kinds, err
}

func (l *Library) Transform(ctx context.Context, params protocol.ToolTransformParams) (string, error) {
	var result protocol.ToolText
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopToolsTransform, params, "ToolText", &result)
	return result.Text, err
}

func (l *Library) QR(ctx context.Context, params protocol.ToolQrParams) (string, error) {
	var result protocol.ToolText
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopToolsQr, params, "ToolText", &result)
	return result.Text, err
}

func (l *Library) OcrEngine(ctx context.Context) (protocol.OcrEngineInfo, error) {
	var result protocol.OcrEngineInfo
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopOcrEngine, protocol.ToolCatalogParams{}, "OcrEngineInfo", &result)
	return result, err
}

func (l *Library) OcrText(ctx context.Context, recognize bool, params protocol.ManualOcrParams) (protocol.ManualOcrText, error) {
	var result protocol.ManualOcrText
	instance := l.owner.Hello().State.ServiceInstanceId
	method := protocol.MethodDesktopOcrStatus
	if recognize {
		if params.ServiceInstanceId != instance {
			return result, client.ErrProtocol
		}
		method = protocol.MethodDesktopOcrRecognize
	} else {
		params.Reveal = false
		params.ServiceInstanceId = instance
	}
	err := l.snapshot(ctx, method, params, "ManualOcrText", &result, params.ExpectedEpoch)
	return result, err
}
