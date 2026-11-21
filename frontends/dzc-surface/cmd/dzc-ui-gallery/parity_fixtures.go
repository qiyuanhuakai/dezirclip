package main

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

func (g *gallery) parityFixture(c *ui.Context, kit uikit.Kit, name string) bool {
	switch name {
	case "cli-settings":
		g.cliSettingsFixture(c, kit)
	case "capture-actions":
		g.captureActionsFixture(c, kit)
	case "capture-engine":
		g.captureEngineFixture(c, kit)
	case "fixed-capture-renderer":
		ui.Column(c).Width(330).Children(func() { kit.FixedCaptureRenderer(c, "Quick Paste interface", "MyGo Native") })
	case "theme-manager":
		g.themeManagerFixture(c, kit)
	case "background-settings":
		g.backgroundSettingsFixture(c, kit)
	case "settings-field":
		ui.Column(c).Width(330).Gap(8).Children(func() {
			kit.SettingsField(c, uikit.SettingsFieldOptions{Stacked: true}).Children(func() {
				kit.SettingsFieldLabel(c, "Interface font", "Setting help: Interface font", "Use the theme font by default.")
				font := "example"
				kit.SearchSelect(c, uikit.SearchSelectOptions{Label: "Interface font", Value: &font, Choices: []uikit.SelectOption{
					{Label: "Default font"}, {Value: "example", Label: "Example Sans"},
				}})
			})
			kit.SettingsField(c, uikit.SettingsFieldOptions{}).Children(func() {
				kit.SettingsFieldLabel(c, "Idle timeout", "", "").Grow(1)
				timeout := "30"
				input, _ := kit.SettingsNumberField(c, &timeout, "Idle timeout", false)
				input.Width(112)
			})
		})
	case "settings-section-card":
		if g.state == "wide" {
			kit.SettingsSectionGrid(c, "General settings", 700, func() {
				kit.SettingsLabel(c, "Start hidden")
				kit.SettingsLabel(c, "Open near pointer")
				path := "C:/Example/Clipboard"
				ui.Column(c).ColumnSpan(-1).Children(func() { kit.TextField(c, &path, "Data path") })
			})
			break
		}
		kit.SettingsSectionCard(c, "General settings", &g.groupOpen, func() {
			kit.SettingsLabel(c, "Keep clipboard history")
		})
	case "settings-choices":
		width := float32(330)
		system, stable, preview := "System", "Stable", "Preview"
		if g.state == "narrow" {
			width = 168
			system, stable, preview = "Follow system appearance", "Stable releases", "Preview releases"
		}
		disabled := g.state == "disabled"
		ui.Column(c).Width(width).Gap(8).Children(func() {
			kit.SettingsLabel(c, "Color mode")
			kit.SettingsChoices(c, "Color mode", g.colorChoice, []uikit.SelectOption{
				{Value: "system", Label: system}, {Value: "light", Label: "Light"}, {Value: "dark", Label: "Dark"},
			}, false, disabled, func(value string) { g.colorChoice = value })
			kit.SettingsLabel(c, "Update channel")
			kit.SettingsChoices(c, "Update channel", g.channelChoice, []uikit.SelectOption{
				{Value: "stable", Label: stable}, {Value: "preview", Label: preview},
			}, true, disabled, func(value string) { g.channelChoice = value })
		})
	case "hotkey-list":
		kit = kit.WithHotkeyPlatform("linux")
		if g.state == "win-v" {
			kit = kit.WithHotkeyPlatform("windows")
		}
		kit.HotkeyListEditor(c, "Global hotkey", func() {
			kit.HotkeyBinding(c, "+ Global hotkey", "", "+ Global hotkey", false, false)
		}, func() {
			kit.HotkeyListWithOptions(c, uikit.HotkeyListOptions{
				Values: g.bindings, Placeholder: "Not set", RemoveLabel: "Delete",
				CanRemove: func(value string) bool { return g.state != "win-v" || value != "Super+V" },
				OnRemove: func(value string) {
					g.bindings = slices.DeleteFunc(g.bindings, func(binding string) bool { return value == binding })
				},
			})
		})
	case "emoji-favorites":
		width, _ := c.Size()
		padding := presets.Select(g.theme, g.mode).ShowcasePadding()
		options := uikit.EmojiFavoritesOptions{ContentWidth: width - padding[1] - padding[3], Count: 1,
			Label: "Favorites", AddLabel: "Add emoji", Hint: "Drag to add emoji", DropLabel: "Drop images here",
			Dragging: g.state == "drop", Item: func(c *ui.Context, _ int, width float32) {
				kit.EmojiFavoriteTile(c, uikit.EmojiFavoriteOptions{
					Name: "Example image", RemoveLabel: "Delete favorite", Width: width,
					OnRemove: func() {},
					Preview:  func() { ui.Text(c, "🌿").PassThrough() },
				})
			}}
		if g.state == "empty" || g.state == "drop" {
			options.Count, options.EmptyTitle, options.Hint = 0, "No emoji yet", "Click or drag images to add quickly"
		}
		ui.Column(c).FillWidth().Height(220).Children(func() {
			kit.EmojiFavorites(c, &g.characterList, options)
		})
	case "settings-data":
		ui.Column(c).Width(330).Gap(8).Children(func() {
			kit.SettingsDataPath(c, "Data storage path", "C:/Example/Documents/Clipboard/profile.db", func() {
				kit.SettingsDataAction(c, "Change", "", false)
				kit.SettingsDataAction(c, "Open", "", false)
			})
			kit.SettingsDataActions(c, func() {
				kit.SettingsDataAction(c, "Export data", "export", false)
				kit.SettingsDataAction(c, "Import data", "import", false)
			})
		})
	case "history-typography":
		g.historyTypographyFixture(c, kit)
	case "history-preview-status":
		g.historyPreviewStatusFixture(c, kit)
	case "history-rich-text":
		kit.HistoryCard(c, uikit.HistoryCardOptions{
			Source: "Editor", Time: "12:30", Summary: "Formatted clipboard card",
			Actions: []uikit.HeaderAction{{Icon: "pin", Label: "Pin"}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}},
			Compact: g.state == "compact",
			Rich: uikit.ParseHistoryRichText(`<p><strong>Release checklist</strong> <em>from the editor</em></p>
<p><u>Review notes</u> and <a href="#details">open the draft.</a></p>
<p>Check the Windows and Linux packages.</p>
<p>Ask for feedback after the preview.</p>
<p>Keep the original file with the notes.</p>`),
		})
	case "backup-dialog":
		g.backupDialogFixture(c, kit)
	case "settings-update":
		ui.Column(c).Width(330).Gap(8).Children(func() {
			kit.UpdateVersionRow(c, "Installed version", "0.2.0-beta.4")
			kit.UpdateVersionRow(c, "Channel version", "0.2.0-beta.5")
			options := uikit.UpdateProgressOptions{Label: "Update download progress", Status: "Downloading update…", Amount: "42%", Fraction: 0.42}
			switch g.state {
			case "installing":
				options.Status, options.Amount, options.Fraction = "Installing and restarting…", "100%", 1
				options.Verified = "Official update signature verified"
			case "unknown-size":
				options.Amount, options.Fraction = "2.5 MiB", -1
			}
			kit.UpdateProgress(c, options)
		})
	case "settings-update-actions":
		ui.Column(c).Width(330).Gap(8).Children(func() {
			busy := g.state == "busy"
			kit.UpdateActions(c, func() {
				kit.UpdateAction(c, uikit.UpdateCheck, "Check now", busy)
				kit.UpdateAction(c, uikit.UpdateInstall, "Install update", busy)
				kit.UpdateAction(c, uikit.UpdateReleases, "Open release page", false)
			})
			if busy {
				kit.UpdateProgress(c, uikit.UpdateProgressOptions{
					Label: "Update download progress", Status: "Downloading update…", Amount: "42%", Fraction: 0.42,
				})
			}
		})
	case "font-select":
		ui.Column(c).Width(350).Gap(6).Padding(6, 0).Children(func() {
			kit.SettingsLabel(c, "Interface font")
			value := "Georgia"
			kit.SearchSelect(c, uikit.SearchSelectOptions{Label: "Interface font", Value: &value,
				Choices: []uikit.SelectOption{{Label: "Default font"},
					{Value: "Georgia", Label: "Georgia", FontFamily: "Georgia, serif"},
					{Value: "Courier New", Label: "Courier New", FontFamily: "Courier New, monospace"}}})
		})
	case "preview-text":
		appearance := *c.Theme()
		appearance.FontSize = 11
		if g.state == "large" {
			appearance.FontSize = 18
		}
		c.SetTheme(&appearance)
		ui.Column(c).Width(365).Height(90).Children(func() {
			uikit.PreviewText(c, "Read the clipboard preview.\nThe second line stays visible.")
		})
	case "search-select":
		scope := ui.Column(c).Width(385).Gap(12)
		label := "Images"
		var choices []uikit.SelectOption
		if g.state == "app-inherited" || g.state == "app-same-name" {
			label = "Code"
			choices = []uikit.SelectOption{{Label: "Follow text setting (Example Editor)"},
				{Value: "C:/Apps/CodeEditor.exe", Label: "Code Editor (C:/Apps/CodeEditor.exe)"},
				{Value: "D:/Portable/CodeEditor.exe", Label: "Code Editor (D:/Portable/CodeEditor.exe)"}}
		} else {
			choices = []uikit.SelectOption{{Label: "System default (Example Browser)"}}
			if g.state == "long-options" {
				scope.Width(260).Margin(0, 0, 0, 150)
				choices = append(choices, uikit.SelectOption{Value: "/example/long", Label: longApplicationLabel})
			}
			choices = append(choices, uikit.SelectOption{Value: "/example/photo-viewer", Label: "Photo Viewer"},
				uikit.SelectOption{Value: "/example/archive", Label: "Archive Manager"})
		}
		scope.Children(func() {
			ui.Column(c).FillWidth().Gap(6).Padding(6, 0).Children(func() {
				kit.SettingsLabel(c, "Interface font")
				font := "Example Sans"
				kit.SearchSelect(c, uikit.SearchSelectOptions{Label: "Interface font", NoMatches: "No matching fonts", Value: &font,
					Choices: []uikit.SelectOption{{Label: "Default font"}, {Value: "Example Sans", Label: "Example Sans"}, {Value: "Example Serif", Label: "Example Serif"}}})
			})
			ui.Column(c).FillWidth().Gap(6).Padding(6, 0).Children(func() {
				kit.SettingsLabel(c, strings.ToUpper(label)).FontSize(11).Opacity(0.8)
				kit.SearchSelect(c, uikit.SearchSelectOptions{Label: label, NoMatches: "No matching applications", Value: &g.choice,
					Choices: choices})
			})
		})
	case "tag-search":
		ui.Column(c).Width(380).Gap(8).Children(func() {
			rail, picker, contents := "", "", "Release notes"
			kit.TagToolbar(c, func() {
				kit.TagNavigation(c, uikit.TagNavigationOptions{Label: "Tags", SearchLabel: "Find tags", Selected: "work",
					Choices: []uikit.TagChoice{{ID: "work", Label: "Work", Count: 4}}})
			}, nil, nil)
			ui.Column(c).Width(156).Children(func() {
				kit.TagSearch(c, uikit.TagSearchOptions{Variant: uikit.TagRailSearch, Query: &rail, Label: "Find a tag"})
			})
			kit.TagSearch(c, uikit.TagSearchOptions{Variant: uikit.TagPickerSearch, Query: &picker, Label: "Find tags"})
			kit.TagSearch(c, uikit.TagSearchOptions{Variant: uikit.TagContentsSearch, Query: &contents,
				Label: "Search tagged entries", ClearLabel: "Clear search"})
		})
	case "tag-toolbar":
		kit.TagToolbar(c, func() {
			kit.TagToolbarTitle(c, "Work")
		}, []uikit.HeaderAction{
			{Icon: "collapse", Label: "Compact view"}, {Icon: "conceal", Label: "Hide entries", Active: true},
			{Icon: "manage", Label: "Manage"}, {Icon: "add", Label: "Add entry"}, {Icon: "more", Label: "Tag actions"},
		}, nil)
	case "tag-history-entry":
		width := float32(360)
		if g.state == "compact" {
			width = 245
		}
		area := ui.Column(c).Width(width)
		selected := ui.Local(area, "tag-entries-selected", func() [2]bool { return [2]bool{false, true} })
		area.Children(func() {
			for i, name := range []string{"Project notes", "Release checklist"} {
				options := uikit.HistoryCardOptions{Source: "Editor", Time: "12:30", Summary: name,
					Width: width, Compact: g.state == "compact",
					Actions: []uikit.HeaderAction{{Icon: "pin", Label: "Pin"}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}}}
				var selection *uikit.TagHistorySelection
				if g.state != "browse" {
					selection = &uikit.TagHistorySelection{Label: "Select entry: " + name, Checked: selected[i],
						OnToggle: func() { selected[i] = !selected[i] }}
				}
				kit.TagHistoryEntry(c, options, selection)
			}
		})
	case "tag-batch-toolbar":
		width := float32(360)
		if g.state == "narrow" {
			width = 205
		}
		area := ui.Column(c).Width(width)
		selected := ui.Local(area, "batch-selected", func() bool { return g.state != "default" })
		area.Children(func() {
			label, count := "Select all", "0 selected"
			if *selected {
				label, count = "Clear selection", "32 selected"
				if g.state == "narrow" {
					label = "Clear all matching entries"
				}
			}
			kit.TagBatchToolbar(c, uikit.TagBatchToolbarOptions{
				SelectLabel: label, CountLabel: count, DeleteLabel: "Delete",
				AllSelected: *selected, DeleteDisabled: !*selected, OnSelect: func() { *selected = !*selected },
			})
		})
	case "privacy-rules":
		exclusionsFirst := g.state == "resized" || g.state == "focus-exclusions"
		ui.Column(c).Width(490).Children(func() {
			drawExclusions := func() {
				ui.Column(c).FillWidth().Gap(6).Padding(8, 0).Children(func() {
					kit.SettingsLabel(c, "Application exclusions")
					value := "Example Editor\nExample Browser"
					kit.SourceExclusionField(c, &value, "Excluded applications", "").ReadOnly(true)
				})
			}
			count := 2
			kit.PrivacyRulesHeading(c, "Privacy rules", &count)
			if exclusionsFirst {
				drawExclusions()
			}
			kit.PrivacyRuleGrid(c, 2, func() {
				kit.PrivacyRuleCard(c, uikit.PrivacyRuleCardOptions{
					Title: "Allowed content", Source: "Example Editor", Scope: "Applies only to matching content.",
					Identifier: "ID a13b4108", Created: "Created 2026-08-18 10:24", RemoveLabel: "Revoke exception",
				})
				kit.PrivacyRuleCard(c, uikit.PrivacyRuleCardOptions{
					Title: "Ignore source", Source: "Example Browser", Scope: "Applies to this application's future entries.",
					Identifier: "ID b27c5419", Created: "Created 2026-08-18 11:35", RemoveLabel: "Remove rule",
					Confirming: true, ConfirmLabel: "Remove privacy rule", Prompt: "Remove the rule for Example Browser?", CancelLabel: "Cancel",
				})
			})
			if !exclusionsFirst {
				drawExclusions()
			}
		})
	case "privacy-detection":
		width := float32(490)
		if g.state == "narrow" || g.state == "resized" {
			width = 290
		}
		panel := ui.Column(c).Width(width).Gap(8)
		options := ui.Local(panel, "detection", func() []uikit.PrivacyDetectionOption {
			return []uikit.PrivacyDetectionOption{
				{Value: "phone", Label: "Phone numbers", Checked: true},
				{Value: "id", Label: "Identity numbers"},
				{Value: "email", Label: "Email addresses", Checked: true},
				{Value: "secrets", Label: "Secrets"},
				{Value: "password", Label: "Strong passwords", Checked: true},
			}
		})
		patterns := ui.Local(panel, "patterns", func() string { return "EXAMPLE-[0-9]+\nproject_[a-z]+" })
		panel.Children(func() {
			kit.SettingsLabel(c, "Privacy detection")
			kit.PrivacyDetectionOptions(c, *options, false, func(value string, checked bool) {
				for index, option := range *options {
					if option.Value == value {
						(*options)[index].Checked = checked
					}
				}
			})
			kit.SettingsLabel(c, "Custom detection patterns")
			kit.PrivacyPatternsField(c, patterns, "Patterns", "One pattern per line")
		})
	case "settings-remove":
		ui.Column(c).Width(330).Gap(8).Children(func() {
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				kit.SettingsLabel(c, "Custom background").Grow(1)
				kit.SettingsRemoveButton(c, "Remove background", true, false)
			})
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				kit.SettingsLabel(c, "Custom theme").Grow(1)
				kit.SettingsRemoveButton(c, "Remove theme", false, false)
			})
		})
	default:
		return false
	}
	return true
}

var historyTypographySamples = []struct{ name, body string }{
	{"short", "Font sample: Wm12 中文"},
	{"wrapped", strings.TrimSpace(strings.Repeat("A clipboard preview can wrap naturally at the card edge. ", 8))},
	{"concealed", "Sensitive content hidden"},
}

func (g *gallery) historyTypographyFixture(c *ui.Context, kit uikit.Kit) {
	size := float32(13)
	switch {
	case g.state == "small":
		size = 11
	case strings.HasSuffix(g.state, "large"):
		size = 18
	case g.state == "custom":
		kit = kit.WithInterfaceFont("Georgia").WithMonospaceFont("Courier New")
	}
	compact := strings.HasPrefix(g.state, "compact")
	gap := float32(4)
	if compact {
		gap = 2
	}
	ui.Column(c).Width(336).Gap(gap).AlignItems(ui.Stretch).Children(func() {
		for _, sample := range historyTypographySamples {
			summary := sample.body
			if sample.name == "wrapped" && strings.HasSuffix(g.state, "token") {
				summary = strings.Repeat("Ab09", 66)
			}
			kit.HistoryCard(c, uikit.HistoryCardOptions{
				Source: "Editor", Time: "12:30", Summary: summary, TextSize: size,
				Compact: compact, Concealed: sample.name == "concealed",
				Actions: []uikit.HeaderAction{{Icon: "pin", Label: "Pin"}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}},
			}).Label(sample.name)
		}
	})
}

var historyPreviewStatusSamples = []struct{ name, label string }{
	{"loading", "Loading preview…"},
	{"empty", "No text preview available"},
	{"error", "Text preview is currently unavailable"},
	{"reference", ""},
	{"pinned", ""},
}

func (g *gallery) historyPreviewStatusFixture(c *ui.Context, kit uikit.Kit) {
	compact := g.state == "compact"
	gap := float32(4)
	if compact {
		gap = 2
	}
	ui.Column(c).Width(336).Gap(gap).AlignItems(ui.Stretch).Children(func() {
		for _, sample := range historyPreviewStatusSamples {
			summary := ""
			if sample.label == "" {
				summary = "A clipboard preview keeps the same width when a card is pinned."
			}
			kit.HistoryCard(c, uikit.HistoryCardOptions{
				Source: "Editor", Time: "12:30", Summary: summary, Placeholder: sample.label,
				Compact: compact, Pinned: sample.name == "pinned",
				Actions: []uikit.HeaderAction{{Icon: "pin", Label: "Pin"}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}},
			}).Label(sample.name)
		}
	})
}

func (g *gallery) backupDialogFixture(c *ui.Context, kit uikit.Kit) {
	if g.state == "glass" {
		ui.Column(c).Padding(112, 0, 0).Children(func() {
			g.parityFixture(c, kit, "settings-data")
		})
	}
	replacing := strings.HasPrefix(g.state, "replace")
	encryptedImport := g.state == "import-encrypted"
	open, restoring := true, g.state == "import" || encryptedImport || replacing
	title, path := "Export clipboard history", ""
	if restoring {
		title, path = "Import clipboard history", "C:/Example/clipboard.json"
	}
	if g.state == "encrypted" || g.state == "error" {
		path = "C:/Example/clipboard.dzc"
	}
	kit.BackupDialog(c, &open, title, false, func() {
		if restoring {
			mode := "merge"
			if replacing {
				mode = "replace"
			}
			kit.BackupChoices(c, "Import mode", &mode, []uikit.SelectOption{
				{Value: "merge", Label: "Merge entries"}, {Value: "replace", Label: "Replace existing entries"},
			}, false)
			if replacing {
				kit.BackupReplaceConfirmation(c, &g.backupReplaceConfirmed, "Confirm replacing clipboard history", false)
			}
		} else {
			format := "json"
			if path != "" {
				format = "encrypted"
			}
			kit.BackupChoices(c, "Export format", &format, []uikit.SelectOption{
				{Value: "json", Label: "JSON (plain text)"}, {Value: "encrypted", Label: "Encrypted backup"},
			}, false)
			if format == "encrypted" {
				var first, second, message string
				if g.state == "error" {
					message = "Passphrase must be at least 12 characters"
				}
				kit.BackupField(c, uikit.BackupFieldOptions{Label: "Encryption passphrase", Value: &first,
					Placeholder: "••••••••••••", Password: true, Error: message})
				kit.BackupField(c, uikit.BackupFieldOptions{Label: "Confirm passphrase", Value: &second,
					Placeholder: "••••••••••••", Password: true})
			}
		}
		kit.BackupField(c, uikit.BackupFieldOptions{Label: "File path", Value: &path, ReadOnly: true, BrowseLabel: "Browse"})
		if encryptedImport {
			var passphrase string
			kit.BackupField(c, uikit.BackupFieldOptions{Label: "Decryption passphrase", Value: &passphrase,
				Placeholder: "••••••••••••", Password: true})
		} else if restoring {
			kit.BackupPreview(c, "Preview entries", []string{"Release checklist", "Example screenshot", "Draft notes"})
		}
	}, func() {
		kit.BackupActions(c, "Cancel", title, false, path == "" || replacing && !g.backupReplaceConfirmed, nil, nil)
	})
}
