package main

import (
	"context"
	"encoding/json"
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

// The Host may scan installed applications for up to ten seconds before saving.
const nativeExternalApplicationTimeout = 15 * time.Second

type nativeApplicationCatalog struct {
	system                  map[string]string
	options                 []uikit.SelectOption
	labelsByPath            map[string]string
	loading, loaded, failed bool
}

func (n *nativeMain) defaultAppsSettings(c *ui.Context) {
	catalog := &n.applications
	if !catalog.loaded && !catalog.loading {
		n.loadApplications()
	}
	if catalog.loading {
		ui.Text(c, n.text("searching_apps")).TextColor(c.Theme().TextMuted).ColumnSpan(-1)
	} else if catalog.failed {
		ui.Row(c).ColumnSpan(-1).Gap(6).Wrap().AlignItems(ui.Center).Children(func() {
			ui.Text(c, n.text("native_application_lookup_failed")).TextColor(c.Theme().TextMuted).Wrap()
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_retry"), Disabled: n.mutating}).Clicked() {
				n.loadApplications()
			}
		})
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
	if inherited := n.preferences["app.text"]; kind == "code" && inherited != "" {
		name := catalog.labelsByPath[inherited]
		if name == "" {
			name = applicationFilename(inherited)
		}
		system = n.text("default_app_follow_text") + " (" + name + ")"
	} else if path := catalog.system[kind]; path != "" {
		system += " (" + applicationFilename(path) + ")"
	}
	options := make([]uikit.SelectOption, 1, len(catalog.options)+2)
	options[0] = uikit.SelectOption{Label: system}
	options = append(options, catalog.options...)

	key := "app." + kind
	path := n.preferences[key]
	selected := path
	if path != "" && catalog.labelsByPath[path] == "" {
		options = append(options, uikit.SelectOption{Value: path, Label: applicationFilename(path)})
	}
	kit := n.mainKit()
	kit.SettingsField(c.Key(key), uikit.SettingsFieldOptions{Stacked: true, Last: kind == "url"}).Children(func() {
		kit.SettingsLabel(c, strings.ToUpper(label)).FontSize(11).Opacity(0.8).Margin(0, 0, 8, 0)
		_, changed := kit.SearchSelect(c, uikit.SearchSelectOptions{
			Label: label, Placeholder: n.text("not_configured"), NoMatches: n.text("no_matching_apps"),
			Value: &selected, Choices: options, Disabled: n.mutating || n.session == nil || n.history.state.ServiceInstanceId == "",
		})
		if path != "" {
			ui.Text(c, applicationFilename(path)).FontSize(10).TextColor(c.Theme().TextMuted).Margin(2, 0, 0).Wrap()
		}
		if changed {
			n.writeSettingWithResult(nativeExternalApplicationTimeout, func(ctx context.Context, epoch uint64) error {
				return n.library.SetSetting(ctx, key, selected, epoch)
			}, func(err error) {
				if err == nil {
					n.preferences[key] = selected
				}
			})
		}
	})
}

func applicationFilename(path string) string {
	name := path[strings.LastIndexAny(path, "/\\")+1:]
	if len(name) >= len(".exe") && strings.EqualFold(name[len(name)-len(".exe"):], ".exe") {
		return name[:len(name)-len(".exe")]
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
			labelsByPath: make(map[string]string, len(listed)),
		}
		names := make(map[string]int, len(listed))
		for _, app := range listed {
			if app.Path == "" || catalog.labelsByPath[app.Path] != "" {
				continue
			}
			label := app.Name
			if label == "" {
				label = applicationFilename(app.Path)
			}
			names[strings.ToLower(label)]++
			catalog.options = append(catalog.options, uikit.SelectOption{Value: app.Path, Label: label})
			catalog.labelsByPath[app.Path] = label
		}
		for index, option := range catalog.options {
			if names[strings.ToLower(option.Label)] > 1 {
				catalog.options[index].Label += " (" + option.Value + ")"
			}
		}
		window.Update(func() { n.applications = catalog })
	}()
}
