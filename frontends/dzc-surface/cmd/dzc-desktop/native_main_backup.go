package main

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeBackupDialog struct {
	open, restoring, replaceConfirmed bool
	format, mode, path                string
	passphrase, confirmation          string
	message                           string
	preview                           []protocol.BackupPreviewRow
	previewAllowed                    bool
}

func (b *nativeBackupDialog) clearPreview() bool {
	hadPreview := len(b.preview) != 0
	b.preview, b.previewAllowed = nil, false
	return hadPreview
}

func (n *nativeMain) closeBackup() {
	if n.backup != nil {
		n.backup.clearPreview()
		n.backup.passphrase, n.backup.confirmation = "", ""
		n.backup = nil
	}
}

func (b *nativeBackupDialog) encrypted() bool {
	if !b.restoring {
		return b.format == "encrypted"
	}
	switch strings.ToLower(filepath.Ext(b.path)) {
	case ".dzc", ".tiez":
		return true
	default:
		return false
	}
}

func (n *nativeMain) dataSettings(c *ui.Context) {
	n.profileSettings(c)
	ready := n.session != nil && n.history.state.ServiceInstanceId != "" && !n.mutating
	ui.Row(c).Gap(6).Wrap().Children(func() {
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("data.export.title"), Disabled: !ready || n.private}).Clicked() {
			n.backup = &nativeBackupDialog{open: true, format: "json"}
			n.notice = ""
		}
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("data.import.title"), Disabled: !ready || n.private}).Clicked() {
			n.backup = &nativeBackupDialog{open: true, restoring: true, mode: "merge"}
			n.notice = ""
		}
	})
	if n.private {
		ui.Text(c, n.text("native_backup_private")).TextColor(c.Theme().TextMuted).Wrap()
	}
}

func (n *nativeMain) backupView(c *ui.Context, kit uikit.Kit, form *nativeBackupDialog) {
	title := n.text("data.export.title")
	if form.restoring {
		title = n.text("data.import.title")
	}
	kit.Dialog(c, &form.open, title, n.text("common_close"), n.mutating, func() {
		_, height := c.Size()
		ui.Scroll(c).FillWidth().MaxHeight(max(72, height-126)).Children(func() {
			ui.Column(c).FillWidth().Gap(7).Children(func() {
				if form.restoring {
					n.backupImportFields(c, kit, form)
				} else {
					n.backupExportFields(c, kit, form)
				}
				ui.Row(c).Gap(6).Wrap().AlignItems(ui.Center).Children(func() {
					if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("browse_file"), Disabled: n.mutating}).Clicked() {
						n.chooseBackupFile(form, title)
					}
					if form.path != "" {
						ui.Text(c, filepath.Base(form.path)).Grow(1).Tooltip(form.path).Wrap().MaxLines(2)
					}
				})
				if len(form.preview) != 0 {
					ui.Text(c, n.text("data.import.preview")).FontWeight(600)
					ui.Scroll(c).MaxHeight(110).Children(func() {
						ui.Column(c).FillWidth().Gap(4).Children(func() {
							for _, row := range form.preview {
								ui.Text(c, row.Content).Wrap().MaxLines(3)
							}
						})
					})
				}
				if form.message != "" {
					ui.Text(c, form.message).TextColor(c.Theme().Danger).Wrap()
				}
			})
		})
		ui.Row(c).Gap(6).Wrap().Justify(ui.End).Children(func() {
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_cancel"), Disabled: n.mutating}).Clicked() {
				form.open = false
			}
			label := n.text("native_export_backup")
			if form.restoring {
				label = n.text("native_import_backup")
			}
			disabled := n.mutating || form.path == "" || form.mode == "replace" && !form.replaceConfirmed
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: label, Disabled: disabled}).Clicked() {
				n.submitBackup(form)
			}
		})
	})
	if !form.open {
		n.closeBackup()
	}
}

func (n *nativeMain) backupImportFields(c *ui.Context, kit uikit.Kit, form *nativeBackupDialog) {
	mode := n.text("data.import.mode")
	ui.Text(c, mode).FontWeight(600)
	if kit.Select(c, &form.mode, mode, []uikit.SelectOption{
		{Value: "merge", Label: n.text("data.import.merge")},
		{Value: "replace", Label: n.text("data.import.replace")},
	}, n.mutating).Changed() {
		form.replaceConfirmed = false
		form.message = ""
	}
	if form.mode == "replace" {
		kit.Checkbox(c, &form.replaceConfirmed, n.text("native_backup_replace_confirm")).Disabled(n.mutating)
	}
	if form.encrypted() {
		n.backupPassword(c, &form.passphrase, n.text("data.import.passphrase_prompt"))
	}
}

func (n *nativeMain) backupExportFields(c *ui.Context, kit uikit.Kit, form *nativeBackupDialog) {
	format := n.text("data.export.choose_format")
	ui.Text(c, format).FontWeight(600)
	if kit.Select(c, &form.format, format, []uikit.SelectOption{
		{Value: "json", Label: n.text("data.export.json")},
		{Value: "encrypted", Label: n.text("data.export.encrypted")},
	}, n.mutating).Changed() {
		form.path, form.message, form.passphrase, form.confirmation = "", "", "", ""
	}
	if form.encrypted() {
		n.backupPassword(c, &form.passphrase, n.text("data.export.passphrase_prompt"))
		n.backupPassword(c, &form.confirmation, n.text("data.export.confirm_passphrase"))
	}
}

func (n *nativeMain) backupPassword(c *ui.Context, value *string, label string) {
	ui.Text(c, label).FontWeight(600)
	n.mainKit().TextField(c, value, label).FillWidth().Password().ReadOnly(n.mutating)
}

func (n *nativeMain) chooseBackupFile(form *nativeBackupDialog, title string) {
	if n.session == nil || n.backup != form || n.mutating || n.private {
		return
	}
	form.clearPreview()
	n.mutating, form.message = true, ""
	session, window, language := n.session, n.window, n.language()
	restoring, encrypted := form.restoring, form.encrypted()
	n.shell.dock.dialogs.Add(1)
	go func() {
		path, err := pickBackupFile(window, title, language, restoring, encrypted)
		n.shell.dock.dialogs.Add(-1)
		n.update(session, func() {
			n.mutating = false
			if n.backup != form {
				return
			}
			if window.IsVisible() && !window.IsMinimized() && !window.IsFocused() {
				window.Focus()
			}
			if err != nil {
				form.message = n.text("native_backup_picker_failed")
				return
			}
			if path == "" {
				return
			}
			form.path, form.replaceConfirmed = path, false
			if restoring {
				form.passphrase = ""
				if !form.encrypted() {
					n.previewBackup(form)
				}
			}
		})
	}()
}

func pickBackupFile(window *mygo.Window, title string, language nativeLanguage, restoring, encrypted bool) (string, error) {
	if restoring {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: window, Title: title, Filters: []mygo.FileFilter{
			{Name: language.text("native_backup_files"), Extensions: []string{"json", "dzc", "tiez"}},
		}})
		if err != nil || len(paths) == 0 {
			return "", err
		}
		return paths[0], nil
	}
	filter := mygo.FileFilter{Name: language.text("data.export.json"), Extensions: []string{"json"}}
	if encrypted {
		filter = mygo.FileFilter{Name: language.text("data.export.encrypted"), Extensions: []string{"dzc"}}
	}
	return mygo.Dialog.Save(mygo.SaveDialogOptions{Parent: window, Title: title,
		DefaultPath: "clipboard-export." + filter.Extensions[0], Filters: []mygo.FileFilter{filter}})
}

func (n *nativeMain) previewBackup(form *nativeBackupDialog) {
	n.mutating, form.previewAllowed = true, true
	session, epoch, path := n.session, n.history.state.PrivacyEpoch, form.path
	go func() {
		ctx, cancel := context.WithTimeout(session, 8*time.Second)
		defer cancel()
		rows, err := n.library.PreviewBackup(ctx, epoch, path)
		n.update(session, func() {
			n.mutating = false
			if n.backup != form || !form.previewAllowed {
				return
			}
			if err != nil {
				form.message = n.text("native_backup_preview_failed")
			} else {
				form.preview = rows
			}
		})
	}()
}

func (n *nativeMain) submitBackup(form *nativeBackupDialog) {
	if n.session == nil || n.backup != form || n.mutating || n.private || form.path == "" ||
		form.mode == "replace" && !form.replaceConfirmed {
		return
	}
	var password *string
	if form.encrypted() {
		switch {
		case form.restoring && form.passphrase == "":
			form.message = n.text("native_backup_password_required")
			return
		case !form.restoring && utf8.RuneCountInString(form.passphrase) < 12:
			form.message = n.text("data.export.passphrase_too_short")
			return
		case !form.restoring && form.passphrase != form.confirmation:
			form.message = n.text("data.export.passphrase_mismatch")
			return
		}
		value := form.passphrase
		password = &value
	}
	n.mutating, form.message = true, ""
	form.clearPreview()
	form.passphrase, form.confirmation = "", ""
	session, epoch := n.session, n.history.state.PrivacyEpoch
	restoring, path, mode, format := form.restoring, form.path, form.mode, form.format
	go func() {
		ctx, cancel := context.WithTimeout(session, 2*time.Minute)
		defer cancel()
		var err error
		if restoring {
			_, err = n.library.ImportBackup(ctx, epoch, path, mode, password)
		} else {
			_, err = n.library.ExportBackup(ctx, epoch, path, format, password)
		}
		n.update(session, func() {
			n.mutating = false
			if n.backup != form {
				return
			}
			if err != nil {
				form.message = n.backupError(err, restoring)
				return
			}
			n.closeBackup()
			n.notice = n.text("data.export.success")
			if restoring {
				n.notice = n.text("data.import.success")
			}
		})
	}()
}

func (n *nativeMain) backupError(err error, restoring bool) string {
	var remote *client.RemoteError
	if errors.As(err, &remote) {
		switch remote.Code {
		case protocol.WireErrorCodePrivacySessionActive:
			return n.text("native_backup_private")
		case protocol.WireErrorCodeSensitiveAccessDenied:
			if !restoring {
				return n.text("native_backup_protected_export")
			}
		}
	}
	if restoring {
		return n.text("native_backup_import_failed")
	}
	return n.text("native_backup_export_failed")
}
