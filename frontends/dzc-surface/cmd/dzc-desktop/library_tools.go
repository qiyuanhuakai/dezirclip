package main

import (
	"context"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

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
