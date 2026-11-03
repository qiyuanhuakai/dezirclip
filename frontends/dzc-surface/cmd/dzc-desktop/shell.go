package main

import (
	"context"
	"errors"
	"net/url"
	"strings"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
)

type DesktopShell struct{}

func (*DesktopShell) Version() string { return buildinfo.Version }

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
