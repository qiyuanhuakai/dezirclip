package main

import (
	"context"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func (l *Library) ImportTheme(ctx context.Context, path string, epoch uint64) (protocol.DesktopThemePackage, error) {
	params := protocol.ThemeImportParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path}
	var theme protocol.DesktopThemePackage
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopThemesImport, params, "DesktopThemePackage", &theme)
	return theme, err
}

func (l *Library) ExportTheme(ctx context.Context, id, path string, epoch uint64) error {
	params := protocol.ThemeExportParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Id: id, Path: path}
	var result protocol.OperationResult
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopThemesExport, params, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

func (l *Library) RemoveTheme(ctx context.Context, id string, epoch uint64) error {
	params := protocol.ThemeIdParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Id: id}
	var result protocol.ThemeRemoved
	return l.owner.DesktopCall(ctx, protocol.MethodDesktopThemesRemove, params, "ThemeRemoved", &result)
}
