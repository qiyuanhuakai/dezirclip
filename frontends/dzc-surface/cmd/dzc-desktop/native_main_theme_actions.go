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
		if uikit.Button(c, uikit.ButtonOptions{Label: "Import theme", Disabled: disabled || full}).Clicked() {
			n.changeThemeFile(importNativeTheme, protocol.DesktopThemePackage{})
		}
		if selected := n.findImportedTheme(n.theme); selected != nil {
			if uikit.Button(c, uikit.ButtonOptions{Label: "Export theme", Disabled: disabled}).Clicked() {
				n.changeThemeFile(exportNativeTheme, *selected)
			}
			if uikit.Button(c, uikit.ButtonOptions{Label: "Remove theme", Disabled: disabled}).Clicked() {
				n.changeThemeFile(removeNativeTheme, *selected)
			}
		}
	})
	if full {
		ui.Text(c, "The custom theme library is full.").TextColor(c.Theme().TextMuted)
	}
}

func (n *nativeMain) changeThemeFile(action nativeThemeAction, selected protocol.DesktopThemePackage) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating, n.themeWorking, n.notice = true, true, ""
	session, epoch, window := n.session, n.history.state.PrivacyEpoch, n.window
	n.shell.dock.dialogs.Add(1)
	go func() {
		path, confirmed, err := pickThemeFile(window, action, selected)
		n.shell.dock.dialogs.Add(-1)
		var notice string
		refresh := false
		if err != nil {
			notice = "Could not open the theme dialog."
		} else if confirmed && session.Err() == nil {
			ctx, cancel := context.WithTimeout(session, 10*time.Second)
			notice, refresh = n.updateThemeFile(ctx, epoch, action, selected.Id, path)
			cancel()
		}
		n.update(session, func() {
			n.mutating, n.themeWorking = false, false
			if refresh || n.themeRefresh {
				n.prepare(n.resume)
			}
			n.notice = notice
		})
	}()
}

func pickThemeFile(window *mygo.Window, action nativeThemeAction, selected protocol.DesktopThemePackage) (string, bool, error) {
	filters := []mygo.FileFilter{{Name: "DezirClip themes", Extensions: []string{"json"}}}
	switch action {
	case importNativeTheme:
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: window, Title: "Import theme", Filters: filters})
		if err != nil || len(paths) == 0 {
			return "", false, err
		}
		return paths[0], true, nil
	case exportNativeTheme:
		path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
			Parent: window, Title: "Export theme", DefaultPath: selected.Id + ".json", Filters: filters,
		})
		return path, path != "", err
	case removeNativeTheme:
		result, err := mygo.Dialog.Message(mygo.MessageOptions{
			Parent: window, Title: selected.Name, Type: mygo.MessageWarning, Message: "Remove this custom theme?",
			Buttons: []string{"Remove", "Cancel"}, DefaultButton: 1, CancelButton: 1,
		})
		return "", result.Button == 0, err
	}
	return "", false, nil
}

func (n *nativeMain) updateThemeFile(ctx context.Context, epoch uint64, action nativeThemeAction, id, path string) (notice string, refresh bool) {
	switch action {
	case importNativeTheme:
		imported, err := n.library.ImportTheme(ctx, path, epoch)
		if err != nil {
			return "Could not import the theme.", true
		}
		if err := n.library.SetSetting(ctx, "app.theme", "custom:"+imported.Id, epoch); err != nil {
			return "Theme imported. Select it in Appearance to apply it.", true
		}
		return "Theme imported and applied.", true
	case exportNativeTheme:
		if err := n.library.ExportTheme(ctx, id, path, epoch); err != nil {
			return "Could not export the theme.", false
		}
		return "Theme exported.", false
	case removeNativeTheme:
		if err := n.library.RemoveTheme(ctx, id, epoch); err != nil {
			return "Could not remove the theme.", true
		}
		return "Theme removed.", true
	}
	return "", false
}
