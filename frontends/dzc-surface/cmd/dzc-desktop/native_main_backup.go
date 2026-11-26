package main

import (
	"context"
	"errors"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

type nativeBackupDialog struct {
	open, restoring, replaceConfirmed  bool
	format, mode, path                 string
	passphrase, confirmation           string
	passphraseError, confirmationError string
	message                            string
	preview                            []protocol.BackupPreviewRow
	previewAllowed                     bool
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
	return b.format == "encrypted"
}

func (b *nativeBackupDialog) canSubmit() bool {
	return b.path != "" && b.format != "" && (!b.restoring || b.mode != "replace" || b.replaceConfirmed)
}

func (n *nativeMain) dataSettings(c *ui.Context) {
	n.profileSettings(c)
	ready := n.session != nil && n.history.state.ServiceInstanceId != "" && !n.mutating
	kit := n.mainKit()
	kit.SettingsDataActions(c, func() {
		if kit.SettingsDataAction(c, n.text("data.export.title"), "export", !ready || n.private).Clicked() {
			n.backup = &nativeBackupDialog{open: true, format: "json"}
			n.notice = ""
		}
		if kit.SettingsDataAction(c, n.text("data.import.title"), "import", !ready || n.private).Clicked() {
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
	kit.BackupDialog(c, &form.open, title, n.mutating, func() {
		if form.restoring {
			n.backupImportFields(c, kit, form)
		} else {
			n.backupExportFields(c, kit, form)
		}
		kit.BackupField(c, uikit.BackupFieldOptions{
			Label: n.text("data_path"), Value: &form.path, BrowseLabel: n.text("browse_file"),
			ReadOnly: true, Disabled: n.mutating, OnBrowse: func() { n.chooseBackupFile(form, title) },
		})
		if form.restoring && form.encrypted() {
			n.backupPassword(c, kit, &form.passphrase, n.text("data.import.passphrase_prompt"), form.passphraseError)
		}
		if len(form.preview) != 0 {
			entries := make([]string, len(form.preview))
			for i, row := range form.preview {
				entries[i] = row.Content
			}
			kit.BackupPreview(c, n.text("data.import.preview"), entries)
		}
		if form.message != "" {
			kit.BackupError(c, form.message)
		}
	}, func() {
		submit := title
		if n.mutating {
			submit = n.text("processing")
		}
		kit.BackupActions(c, n.text("cancel"), submit, n.mutating, !form.canSubmit(),
			func() { form.open = false }, func() { n.submitBackup(form) })
	})
	if !form.open {
		n.closeBackup()
	}
}

func (n *nativeMain) backupImportFields(c *ui.Context, kit uikit.Kit, form *nativeBackupDialog) {
	if kit.BackupChoices(c, n.text("data.import.mode"), &form.mode, []uikit.SelectOption{
		{Value: "merge", Label: n.text("data.import.merge")},
		{Value: "replace", Label: n.text("data.import.replace")},
	}, n.mutating) {
		form.replaceConfirmed = false
		if form.format != "" {
			form.message = ""
		}
	}
	if form.mode == "replace" {
		kit.BackupReplaceConfirmation(c, &form.replaceConfirmed, n.text("native_backup_replace_confirm"), n.mutating || form.format == "")
	}
}

func (n *nativeMain) backupExportFields(c *ui.Context, kit uikit.Kit, form *nativeBackupDialog) {
	if kit.BackupChoices(c, n.text("data.export.choose_format"), &form.format, []uikit.SelectOption{
		{Value: "json", Label: n.text("data.export.json")},
		{Value: "encrypted", Label: n.text("data.export.encrypted")},
	}, n.mutating) {
		form.path, form.message, form.passphrase, form.confirmation = "", "", "", ""
		form.passphraseError, form.confirmationError = "", ""
	}
	if form.encrypted() {
		n.backupPassword(c, kit, &form.passphrase, n.text("data.export.passphrase_prompt"), form.passphraseError)
		n.backupPassword(c, kit, &form.confirmation, n.text("data.export.confirm_passphrase"), form.confirmationError)
	}
}

func (n *nativeMain) backupPassword(c *ui.Context, kit uikit.Kit, value *string, label, message string) {
	kit.BackupField(c, uikit.BackupFieldOptions{Label: label, Value: value, Placeholder: "••••••••••••",
		Error: message, Password: true, Disabled: n.mutating})
}

func (n *nativeMain) chooseBackupFile(form *nativeBackupDialog, title string) {
	if n.session == nil || n.backup != form || n.mutating || n.private {
		return
	}
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
			form.clearPreview()
			form.path, form.replaceConfirmed = path, false
			if restoring {
				form.format, form.passphrase, form.passphraseError = "", "", ""
				n.inspectBackup(form)
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

func (n *nativeMain) inspectBackup(form *nativeBackupDialog) {
	n.mutating, form.previewAllowed = true, true
	session, epoch, path := n.session, n.history.state.PrivacyEpoch, form.path
	go func() {
		ctx, cancel := context.WithTimeout(session, 8*time.Second)
		defer cancel()
		inspection, err := n.library.InspectBackup(ctx, epoch, path)
		n.update(session, func() {
			n.mutating = false
			if n.backup != form || n.history.state.PrivacyEpoch != epoch {
				return
			}
			if err != nil {
				form.message = n.text("native_backup_preview_failed")
				return
			}
			form.format = "json"
			if inspection.Encrypted {
				form.format = "encrypted"
			}
			if form.previewAllowed {
				form.preview = inspection.Entries
			}
		})
	}()
}

func (n *nativeMain) submitBackup(form *nativeBackupDialog) {
	if n.session == nil || n.backup != form || n.mutating || n.private || !form.canSubmit() {
		return
	}
	var password *string
	form.passphraseError, form.confirmationError, form.message = "", "", ""
	if form.encrypted() {
		if form.restoring && form.passphrase == "" {
			form.passphraseError = n.text("native_backup_password_required")
		}
		if !form.restoring && utf8.RuneCountInString(form.passphrase) < 12 {
			form.passphraseError = n.text("data.export.passphrase_too_short")
		}
		if !form.restoring && form.passphrase != form.confirmation {
			form.confirmationError = n.text("data.export.passphrase_mismatch")
		}
		if form.passphraseError != "" || form.confirmationError != "" {
			return
		}
		value := form.passphrase
		password = &value
	}
	n.mutating, form.message = true, ""
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
