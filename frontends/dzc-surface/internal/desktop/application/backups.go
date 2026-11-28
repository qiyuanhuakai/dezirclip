package application

import (
	"context"
	"path/filepath"
	"slices"
	"strings"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func PreviewBackup(ctx context.Context, owner *client.Client, epoch uint64, path string) ([]protocol.BackupPreviewRow, error) {
	var result protocol.BackupPreview
	params := protocol.BackupPreviewParams{ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path}
	err := owner.DesktopCall(ctx, protocol.MethodDesktopBackupPreview, params, "BackupPreview", &result)
	return result.Entries, err
}

func InspectBackup(ctx context.Context, owner *client.Client, epoch uint64, path string) (protocol.BackupInspection, error) {
	var result protocol.BackupInspection
	var capabilities protocol.CapabilityReport
	if err := owner.DesktopCall(ctx, protocol.MethodCapabilities, struct{}{}, "CapabilityReport", &capabilities); err != nil {
		return result, err
	}
	if !slices.Contains(capabilities.Methods, string(protocol.MethodDesktopBackupInspect)) {
		switch strings.ToLower(filepath.Ext(path)) {
		case ".dzc", ".tiez":
			return protocol.BackupInspection{Encrypted: true, Entries: []protocol.BackupPreviewRow{}}, nil
		}
		var err error
		result.Entries, err = PreviewBackup(ctx, owner, epoch, path)
		return result, err
	}
	params := protocol.BackupPreviewParams{ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path}
	err := owner.DesktopCall(ctx, protocol.MethodDesktopBackupInspect, params, "BackupInspection", &result)
	return result, err
}

func ImportBackup(ctx context.Context, owner *client.Client, epoch uint64, path, mode string, passphrase *string) (protocol.BackupImportResult, error) {
	var result protocol.BackupImportResult
	params := protocol.BackupImportParams{ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path, Mode: mode, Passphrase: passphrase}
	err := owner.DesktopCall(ctx, protocol.MethodDesktopBackupImport, params, "BackupImportResult", &result)
	return result, err
}

func ExportBackup(ctx context.Context, owner *client.Client, epoch uint64, path, format string, passphrase *string) (protocol.BackupExportResult, error) {
	var result protocol.BackupExportResult
	params := protocol.BackupExportParams{ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Path: path, Format: format, Passphrase: passphrase}
	err := owner.DesktopCall(ctx, protocol.MethodDesktopBackupExport, params, "BackupExportResult", &result)
	return result, err
}
