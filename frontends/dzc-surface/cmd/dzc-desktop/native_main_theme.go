package main

import (
	"encoding/json"
	"strings"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-presets"
)

const maxImportedThemes = 32

type nativeTheme struct {
	base        string
	light, dark presets.Preset
}

func (n *nativeMain) configureThemes(selection, catalog string) {
	previous := n.findImportedTheme(n.theme)
	if catalog != n.preferences["app.custom_themes"] {
		n.importedThemes = importedThemeCatalog(catalog)
	} else if selection == n.theme {
		return
	}
	entry := n.findImportedTheme(selection)
	if entry == nil {
		n.importedTheme = nil
		return
	}
	if n.importedTheme != nil && previous != nil && previous.Base == entry.Base && previous.Css == entry.Css {
		return
	}
	n.importedTheme = &nativeTheme{
		base: entry.Base, light: presets.Imported(entry.Base, "light", entry.Css), dark: presets.Imported(entry.Base, "dark", entry.Css),
	}
}

func importedThemeCatalog(catalog string) []protocol.DesktopThemePackage {
	var imported []protocol.DesktopThemePackage
	if json.Unmarshal([]byte(catalog), &imported) != nil || len(imported) > maxImportedThemes {
		return nil
	}
	valid := imported[:0]
	for _, entry := range imported {
		if entry.Format == "dezirclip-theme" && entry.Version == 1 && entry.Id != "" && entry.Name != "" {
			valid = append(valid, entry)
		}
	}
	return valid
}

func (n *nativeMain) findImportedTheme(selection string) *protocol.DesktopThemePackage {
	return selectedImportedTheme(n.importedThemes, selection)
}

func selectedImportedTheme(catalog []protocol.DesktopThemePackage, selection string) *protocol.DesktopThemePackage {
	id, isCustom := strings.CutPrefix(selection, "custom:")
	if !isCustom {
		return nil
	}
	for i := range catalog {
		if catalog[i].Id == id {
			return &catalog[i]
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
