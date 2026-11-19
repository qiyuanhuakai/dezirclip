package main

import (
	"encoding/json"
	"strings"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

type nativeTheme struct {
	base        string
	light, dark presets.Preset
}

func (n *nativeMain) configureThemes(selection, catalog string) {
	if catalog != n.preferences["app.custom_themes"] {
		var imported []protocol.DesktopThemePackage
		n.importedThemes = nil
		if json.Unmarshal([]byte(catalog), &imported) == nil && len(imported) <= 32 {
			for _, entry := range imported {
				if entry.Format == "dezirclip-theme" && entry.Version == 1 && entry.Id != "" && entry.Name != "" {
					n.importedThemes = append(n.importedThemes, entry)
				}
			}
		}
	} else if selection == n.theme {
		return
	}
	n.importedTheme = nil
	id, isCustom := strings.CutPrefix(selection, "custom:")
	if !isCustom {
		return
	}
	for _, entry := range n.importedThemes {
		if entry.Id == id {
			n.importedTheme = &nativeTheme{
				base: entry.Base, light: presets.Imported(entry.Base, "light", entry.Css), dark: presets.Imported(entry.Base, "dark", entry.Css),
			}
			return
		}
	}
}

func (n *nativeMain) themeBase() string {
	if n.importedTheme != nil {
		return n.importedTheme.base
	}
	if n.theme == "" || strings.HasPrefix(n.theme, "custom:") {
		return "mica"
	}
	return n.theme
}

func (n *nativeMain) preset() presets.Preset {
	if n.importedTheme != nil {
		if n.mode == "dark" {
			return n.importedTheme.dark
		}
		return n.importedTheme.light
	}
	return presets.Select(n.themeBase(), n.mode)
}
