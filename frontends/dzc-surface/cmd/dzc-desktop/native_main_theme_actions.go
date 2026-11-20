package main

import (
	"context"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeThemeAction uint8

const (
	importNativeTheme nativeThemeAction = iota
	exportNativeTheme
	removeNativeTheme
)

func (n *nativeMain) themeActions(c *ui.Context) {
	disabled := n.mutating || n.session == nil || n.history.state.ServiceInstanceId == ""
	full := len(n.importedThemes) >= maxImportedThemes
	ui.Row(c).Gap(6).Wrap().Children(func() {
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("theme_import"), Disabled: disabled || full}).Clicked() {
			n.changeThemeFile(importNativeTheme, protocol.DesktopThemePackage{})
		}
		if selected := n.findImportedTheme(n.theme); selected != nil {
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("theme_export"), Disabled: disabled}).Clicked() {
				n.changeThemeFile(exportNativeTheme, *selected)
			}
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("theme_delete", "Remove theme"), Disabled: disabled}).Clicked() {
				n.changeThemeFile(removeNativeTheme, *selected)
			}
		}
	})
	if full {
		ui.Text(c, n.text("theme_limit_reached")).TextColor(c.Theme().TextMuted)
	}
}

func (n *nativeMain) changeThemeFile(action nativeThemeAction, selected protocol.DesktopThemePackage) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating, n.appearanceBusy, n.notice = true, true, ""
	session, epoch, window := n.session, n.history.state.PrivacyEpoch, n.window
	language := n.language()
	n.shell.dock.dialogs.Add(1)
	go func() {
		path, confirmed, err := pickThemeFile(window, language, action, selected)
		n.shell.dock.dialogs.Add(-1)
		var notice string
		refresh := false
		if err != nil {
			notice = language.text("theme_operation_failed", "Could not open the theme dialog.")
		} else if confirmed && session.Err() == nil {
			ctx, cancel := context.WithTimeout(session, 10*time.Second)
			notice, refresh = n.updateThemeFile(ctx, epoch, action, selected.Id, path, language)
			cancel()
		}
		n.update(session, func() {
			n.mutating, n.appearanceBusy = false, false
			if refresh || n.appearanceSync {
				n.prepare(n.resume)
			}
			n.notice = notice
		})
	}()
}

func pickThemeFile(window *mygo.Window, language nativeLanguage, action nativeThemeAction, selected protocol.DesktopThemePackage) (string, bool, error) {
	filters := []mygo.FileFilter{{Name: language.text("native_theme_filter"), Extensions: []string{"json"}}}
	switch action {
	case importNativeTheme:
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: window, Title: language.text("theme_import"), Filters: filters})
		if err != nil || len(paths) == 0 {
			return "", false, err
		}
		return paths[0], true, nil
	case exportNativeTheme:
		path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
			Parent: window, Title: language.text("theme_export"), DefaultPath: selected.Id + ".json", Filters: filters,
		})
		return path, path != "", err
	case removeNativeTheme:
		result, err := mygo.Dialog.Message(mygo.MessageOptions{
			Parent: window, Title: selected.Name, Type: mygo.MessageWarning, Message: language.text("theme_delete_confirm"),
			Buttons: []string{language.text("native_remove"), language.text("common_cancel")}, DefaultButton: 1, CancelButton: 1,
		})
		return "", result.Button == 0, err
	}
	return "", false, nil
}

func (n *nativeMain) updateThemeFile(ctx context.Context, epoch uint64, action nativeThemeAction, id, path string, language nativeLanguage) (notice string, refresh bool) {
	switch action {
	case importNativeTheme:
		imported, err := n.library.ImportTheme(ctx, path, epoch)
		if err != nil {
			return language.text("theme_operation_failed", "Could not import the theme."), true
		}
		if err := n.library.SetSetting(ctx, "app.theme", "custom:"+imported.Id, epoch); err != nil {
			return language.text("native_theme_apply_failed"), true
		}
		return language.text("native_theme_applied"), true
	case exportNativeTheme:
		if err := n.library.ExportTheme(ctx, id, path, epoch); err != nil {
			return language.text("theme_operation_failed", "Could not export the theme."), false
		}
		return language.text("theme_exported", "Theme exported."), false
	case removeNativeTheme:
		if err := n.library.RemoveTheme(ctx, id, epoch); err != nil {
			return language.text("theme_operation_failed", "Could not remove the theme."), true
		}
		return language.text("native_theme_removed"), true
	}
	return "", false
}
