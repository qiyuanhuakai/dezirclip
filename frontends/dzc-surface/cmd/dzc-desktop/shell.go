package main

import (
	"context"
	"errors"
	"net/url"
	"strings"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
)

type DesktopShell struct{}

type platformInfo struct {
	Platform  string `json:"platform"`
	Windows10 bool   `json:"is_windows_10"`
	Windows11 bool   `json:"is_windows_11"`
	Linux     bool   `json:"is_linux"`
}

func (shell *DesktopShell) ApplyAppearance(ctx context.Context, theme, mode string, border, mediaDark bool) error {
	w := mygo.CallerWindow(ctx)
	if w == nil {
		return errors.New("WindowUnavailable")
	}
	if mode == "system" {
		mode = shell.SystemThemeMode(ctx)
		if mode == "system" && mediaDark {
			mode = "dark"
		}
	}
	return applyWindowAppearance(w, theme, mode, border)
}

var focusChanged = mygo.NewEvent[bool]("window-focus")
var visibilityChanged = mygo.NewEvent[bool]("window-visibility")
var windowMoved = mygo.NewEvent[bool]("window-moved")
var filesDropped = mygo.NewEvent[[]string]("window-file-drop")

func (*DesktopShell) IsVisible(ctx context.Context) bool {
	w := mygo.CallerWindow(ctx)
	return w != nil && w.IsVisible()
}

func (*DesktopShell) IsFocused(ctx context.Context) bool {
	w := mygo.CallerWindow(ctx)
	return w != nil && w.IsFocused()
}

func (*DesktopShell) Focus(ctx context.Context) error {
	w := mygo.CallerWindow(ctx)
	if w == nil || !w.IsVisible() {
		return errors.New("WindowUnavailable")
	}
	w.Focus()
	return nil
}

func (*DesktopShell) BuildInfo() any { return buildinfo.DesktopReport() }

func (l *Library) ProfileInfo(ctx context.Context) (protocol.DesktopProfileInfo, error) {
	var result protocol.DesktopProfileInfo
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopProfileInfo, protocol.ToolCatalogParams{}, "DesktopProfileInfo", &result)
	return result, err
}

func (l *Library) OpenDataFolder(ctx context.Context) error {
	profile, err := l.ProfileInfo(ctx)
	if err != nil {
		return err
	}
	return mygo.Shell.OpenPath(profile.DataDirectory)
}

func (*DesktopShell) Version() string { return buildinfo.Version }

func (*DesktopShell) SetAlwaysOnTop(ctx context.Context, pinned bool) error {
	w := mygo.CallerWindow(ctx)
	if w == nil {
		return errors.New("WindowUnavailable")
	}
	w.SetAlwaysOnTop(pinned)
	return nil
}

func (*DesktopShell) OpenURL(raw string) error {
	link, err := url.Parse(raw)
	if err != nil || link.Scheme != "https" || link.Host != "github.com" || link.User != nil ||
		(link.Path != "/qiyuanhuakai/dezirclip" && !strings.HasPrefix(link.Path, "/qiyuanhuakai/dezirclip/")) {
		return errors.New("unsupported application link")
	}
	return mygo.Shell.OpenExternal(raw)
}

type fileDialog struct {
	Title       string         `json:"title"`
	DefaultPath string         `json:"defaultPath"`
	Directory   bool           `json:"directory"`
	Multiple    bool           `json:"multiple"`
	Filters     []dialogFilter `json:"filters"`
}

type dialogFilter struct {
	Name       string   `json:"name"`
	Extensions []string `json:"extensions"`
}

func (d fileDialog) nativeFilters() []mygo.FileFilter {
	filters := make([]mygo.FileFilter, len(d.Filters))
	for i, filter := range d.Filters {
		filters[i] = mygo.FileFilter{Name: filter.Name, Extensions: filter.Extensions}
	}
	return filters
}

func (*DesktopShell) Open(ctx context.Context, options fileDialog) ([]string, error) {
	return mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: mygo.CallerWindow(ctx), Title: options.Title,
		DefaultPath: options.DefaultPath, Directory: options.Directory, Multiple: options.Multiple, Filters: options.nativeFilters()})
}

func (*DesktopShell) Save(ctx context.Context, options fileDialog) (string, error) {
	return mygo.Dialog.Save(mygo.SaveDialogOptions{Parent: mygo.CallerWindow(ctx), Title: options.Title,
		DefaultPath: options.DefaultPath, Filters: options.nativeFilters()})
}

func (*DesktopShell) Message(ctx context.Context, text, title string, kind mygo.MessageType, labels []string, cancel int) (int, error) {
	result, err := mygo.Dialog.Message(mygo.MessageOptions{Parent: mygo.CallerWindow(ctx), Title: title, Message: text,
		Type: kind, Buttons: labels, CancelButton: cancel})
	return result.Button, err
}
