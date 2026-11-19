package main

import (
	"encoding/json"
	"strings"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

const maxImportedThemes = 32

type nativeTheme struct {
	base        string
	light, dark presets.Preset
}

func (n *nativeMain) configureThemes(selection, catalog string) {
	if catalog != n.preferences["app.custom_themes"] {
		var imported []protocol.DesktopThemePackage
		n.importedThemes = nil
		if json.Unmarshal([]byte(catalog), &imported) == nil && len(imported) <= maxImportedThemes {
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
	if entry := n.findImportedTheme(selection); entry != nil {
		n.importedTheme = &nativeTheme{
			base: entry.Base, light: presets.Imported(entry.Base, "light", entry.Css), dark: presets.Imported(entry.Base, "dark", entry.Css),
		}
	}
}

func (n *nativeMain) findImportedTheme(selection string) *protocol.DesktopThemePackage {
	id, isCustom := strings.CutPrefix(selection, "custom:")
	if !isCustom {
		return nil
	}
	for i := range n.importedThemes {
		if n.importedThemes[i].Id == id {
			return &n.importedThemes[i]
		}
	}
	return nil
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
