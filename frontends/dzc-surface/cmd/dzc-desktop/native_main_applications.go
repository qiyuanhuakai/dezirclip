package main

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeInstalledApplication struct {
	Name string `json:"name"`
	Path string `json:"path"`
}

type nativeApplicationCatalog struct {
	system                  map[string]string
	labels                  []string
	labelsByPath            map[string]string
	pathsByLabel            map[string]string
	loading, loaded, failed bool
}

func (n *nativeMain) defaultAppsSettings(c *ui.Context) {
	catalog := &n.applications
	if !catalog.loaded && !catalog.loading {
		n.loadApplications()
	}
	if catalog.loading {
		ui.Text(c, n.text("searching_apps")).TextColor(c.Theme().TextMuted)
	} else if catalog.failed {
		ui.Text(c, n.text("native_application_lookup_failed")).TextColor(c.Theme().TextMuted).Wrap()
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_retry"), Disabled: n.mutating}).Clicked() {
			n.loadApplications()
		}
	}
	for _, item := range []struct{ kind, label string }{
		{"text", n.text("type_text")}, {"rich_text", n.text("type_rich_text")}, {"image", n.text("type_image")},
		{"video", n.text("type_video")}, {"code", n.text("type_code")}, {"url", n.text("type_url")},
	} {
		n.applicationSetting(c, item.kind, item.label)
	}
}

func (n *nativeMain) applicationSetting(c *ui.Context, kind, label string) {
	catalog := &n.applications
	system := n.text("system_default")
	if path := catalog.system[kind]; path != "" {
		system += " (" + applicationFilename(path) + ")"
	}
	for catalog.pathsByLabel[system] != "" {
		system += " · " + n.text("system_default")
	}
	options := make([]string, 0, len(catalog.labels)+2)
	options = append(options, system)
	options = append(options, catalog.labels...)

	key := "app." + kind
	path := n.preferences[key]
	selected := system
	if path != "" {
		selected = catalog.labelsByPath[path]
		if selected == "" {
			selected = applicationFilename(path)
			for selected == system || catalog.pathsByLabel[selected] != "" {
				selected += " (" + path + ")"
			}
			options = append(options, selected)
		}
	}
	before := selected
	ui.Text(c, label).FontWeight(600)
	field := n.mainKit().Combo(c, &selected, label, options, n.mutating || n.session == nil || n.history.state.ServiceInstanceId == "").FillWidth().
		Disabled(n.mutating || n.session == nil || n.history.state.ServiceInstanceId == "")
	if field.Changed() && selected != before {
		if selected == system {
			n.saveSetting(key, "")
		} else if choice := catalog.pathsByLabel[selected]; choice != "" {
			n.saveSetting(key, choice)
		}
	}
}

func applicationFilename(path string) string {
	name := filepath.Base(path)
	if extension := filepath.Ext(name); strings.EqualFold(extension, ".exe") {
		return strings.TrimSuffix(name, extension)
	}
	return name
}

func (n *nativeMain) loadApplications() {
	window := n.window
	if window == nil || n.applications.loading {
		return
	}
	n.applications.loading = true
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()
		var listed []nativeInstalledApplication
		var system map[string]string
		apps, appsError := client.PackageTool(ctx, "installed-apps")
		if appsError == nil {
			appsError = json.Unmarshal(apps, &listed)
		}
		defaults, defaultsError := client.PackageTool(ctx, "default-apps")
		if defaultsError == nil {
			defaultsError = json.Unmarshal(defaults, &system)
		}
		catalog := nativeApplicationCatalog{
			system: system, loaded: true, failed: appsError != nil || defaultsError != nil,
			labelsByPath: make(map[string]string, len(listed)), pathsByLabel: make(map[string]string, len(listed)),
		}
		names := make(map[string]int, len(listed))
		for _, app := range listed {
			names[app.Name]++
		}
		for _, app := range listed {
			if app.Path == "" || catalog.labelsByPath[app.Path] != "" {
				continue
			}
			label := app.Name
			if label == "" {
				label = applicationFilename(app.Path)
			}
			if names[app.Name] > 1 {
				label += " (" + app.Path + ")"
			}
			for catalog.pathsByLabel[label] != "" {
				label += " (" + app.Path + ")"
			}
			catalog.labels = append(catalog.labels, label)
			catalog.labelsByPath[app.Path], catalog.pathsByLabel[label] = label, app.Path
		}
		window.Update(func() { n.applications = catalog })
	}()
}
