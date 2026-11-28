package main

import (
	"context"
	"encoding/json"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/desktop/application"
)

func (*DesktopShell) PackageTool(ctx context.Context, action string) (json.RawMessage, error) {
	return client.PackageTool(ctx, action)
}

func (l *Library) Background(ctx context.Context, epoch uint64, expected string) (*string, error) {
	var result protocol.BackgroundResource
	params := protocol.BackgroundParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, ExpectedPath: expected}
	err := l.snapshot(ctx, protocol.MethodDesktopAppearanceBackground, params, "BackgroundResource", &result, epoch)
	return result.Src, err
}

// Reauthorize a background already held by this WebView without transferring its pixels again.
func (l *Library) BackgroundSelected(ctx context.Context, epoch uint64, expected string) (bool, error) {
	var result protocol.DesktopPreferences
	params := protocol.LibraryStateParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	if err := l.snapshot(ctx, protocol.MethodDesktopSettingsPreferences, params, "DesktopPreferences", &result, epoch); err != nil {
		return false, err
	}
	var settings map[string]string
	if err := json.Unmarshal(result.Values, &settings); err != nil {
		return false, err
	}
	return expected != "" && settings["app.custom_background"] == expected, nil
}

func (l *Library) PreviewBackup(ctx context.Context, epoch uint64, path string) ([]protocol.BackupPreviewRow, error) {
	return application.PreviewBackup(ctx, l.owner, epoch, path)
}

func (l *Library) InspectBackup(ctx context.Context, epoch uint64, path string) (protocol.BackupInspection, error) {
	return application.InspectBackup(ctx, l.owner, epoch, path)
}

func (l *Library) ImportBackup(ctx context.Context, epoch uint64, path, mode string, passphrase *string) (protocol.BackupImportResult, error) {
	return application.ImportBackup(ctx, l.owner, epoch, path, mode, passphrase)
}

func (l *Library) ExportBackup(ctx context.Context, epoch uint64, path, format string, passphrase *string) (protocol.BackupExportResult, error) {
	return application.ExportBackup(ctx, l.owner, epoch, path, format, passphrase)
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
	return application.EmojiFavorites(ctx, l.owner, epoch, path)
}

func (l *Library) MutateFavorite(ctx context.Context, epoch uint64, action, path string) (string, error) {
	return application.MutateFavorite(ctx, l.owner, epoch, action, path)
}

func (l *Library) ImportFavoriteData(ctx context.Context, epoch uint64, dataURL string) (string, error) {
	return application.ImportFavoriteData(ctx, l.owner, epoch, dataURL)
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

func (l *Library) CopyTransformed(ctx context.Context, params protocol.DesktopTransformCopyParams) error {
	var result protocol.OperationResult
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopClipboardTransformCopy, params, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

func (l *Library) QR(ctx context.Context, params protocol.ToolQrParams) (string, error) {
	var result protocol.ToolText
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopToolsQr, params, "ToolText", &result)
	return result.Text, err
}

func (l *Library) historyQR(ctx context.Context, params protocol.DesktopEntryParams) (protocol.DesktopQrPreview, error) {
	var preview protocol.DesktopQrPreview
	err := l.snapshot(ctx, protocol.MethodDesktopHistoryQr, params, "DesktopQrPreview", &preview, params.ExpectedEpoch)
	return preview, err
}

func (l *Library) copyHistoryQR(ctx context.Context, params protocol.DesktopQrCopyParams) error {
	var result protocol.OperationResult
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopClipboardQrCopy, params, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
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
		if params.ServiceInstanceId != "" && params.ServiceInstanceId != instance {
			return result, client.ErrProtocol
		}
		params.Reveal = false
		params.ServiceInstanceId = instance
	}
	err := l.snapshot(ctx, method, params, "ManualOcrText", &result, params.ExpectedEpoch)
	return result, err
}

func (l *Library) copyOCRText(ctx context.Context, params protocol.ManualOcrParams) error {
	var result protocol.OperationResult
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopClipboardOcrCopy, params, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

func (l *Library) recognizeForNative(ctx context.Context, params protocol.ManualOcrParams) error {
	var result protocol.ManualOcrText
	var noText uint32
	params.MaxTextBytes = &noText
	return l.readSnapshot(ctx, protocol.MethodDesktopOcrRecognize, params, "ManualOcrText", &result, params.ExpectedEpoch)
}

func (l *Library) nativeOCRPreview(ctx context.Context, params protocol.ManualOcrParams) (protocol.ManualOcrText, error) {
	var result protocol.ManualOcrText
	maximum := uint32(16 * 1024)
	params.MaxTextBytes = &maximum
	err := l.snapshot(ctx, protocol.MethodDesktopOcrStatus, params, "ManualOcrText", &result, params.ExpectedEpoch)
	return result, err
}
