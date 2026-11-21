package uikit

import "github.com/egoist/mygo/ui"

type CLISettingField struct {
	Label, Value string
	Path         bool
}

type CLISettingGuide struct {
	Label, Path, Fallback string
	Disabled              bool
	Open                  func()
}

type CLISettingsStatus struct {
	Label, Value, Action string
	Available, Disabled  bool
	Run                  func()
}

type CLISettingsOptions struct {
	Title, Description, HelpLabel                  string
	Fields, Conflict                               []CLISettingField
	Status                                         CLISettingsStatus
	Notice                                         string
	GuidesTitle, GuidesDescription, GuideOpenLabel string
	Guides                                         []CLISettingGuide
}

var cliGuideFolder = icon("FolderOpen")

// CLISettings renders CLI discovery, PATH status and actions for packaged guides.
func (k Kit) CLISettings(c *ui.Context, options CLISettingsOptions) {
	k.cliSettingsHeading(c, options.Title, options.Description, options.HelpLabel)
	for _, field := range options.Fields {
		k.cliSettingValue(c, field)
	}
	status := options.Status
	k.cliSettingsRow(c.Key("cli-status")).ColumnSpan(-1).Children(func() {
		k.captureSettingsLabel(c, status.Label)
		ui.Row(c).Wrap().AlignItems(ui.Center).Justify(ui.End).Gap(8).MinWidth(0).Children(func() {
			k.SettingsStatusBadge(c, status.Value, status.Available)
			if status.Action != "" && k.settingsAction(c, status.Action, status.Disabled, 28, &[4]float32{5, 10, 5, 10}, nil).Clicked() && status.Run != nil {
				status.Run()
			}
		})
	})
	for _, field := range options.Conflict {
		k.cliSettingValue(c, field)
	}
	if options.Notice != "" {
		k.SettingsField(c.Key("cli-notice"), SettingsFieldOptions{Detail: true, Last: true}).ColumnSpan(-1).Children(func() {
			k.cliSettingText(c, options.Notice).Role(ui.RoleStatus)
		})
	}
	k.cliSettingsHeading(c, options.GuidesTitle, options.GuidesDescription, options.HelpLabel)
	for _, guide := range options.Guides {
		k.cliSettingsRow(c.Key(guide.Fallback)).ColumnSpan(-1).Children(func() {
			ui.Column(c).MinWidth(0).Grow(1).Children(func() {
				k.SettingsLabel(c, guide.Label).Wrap()
				if guide.Path == "" {
					k.cliSettingText(c, guide.Fallback)
				} else {
					k.cliSettingText(c, guide.Path).Tooltip(guide.Path).Selectable()
				}
			})
			label := options.GuideOpenLabel + ": " + guide.Label
			button := k.settingsAction(c, "", guide.Path == "" || guide.Disabled, 28, &[4]float32{5, 10, 5, 10}, cliGuideFolder)
			if button.Label(label).Tooltip(options.GuideOpenLabel).Clicked() && guide.Open != nil {
				guide.Open()
			}
		})
	}
}

func (k Kit) cliSettingsHeading(c *ui.Context, title, description, helpLabel string) {
	k.SettingsField(c.Key(title), SettingsFieldOptions{}).MinHeight(34).ColumnSpan(-1).Children(func() {
		k.SettingsFieldLabel(c, title, helpLabel+": "+title, description).Grow(1)
	})
}

func (k Kit) cliSettingValue(c *ui.Context, field CLISettingField) {
	row := k.cliSettingsRow(c.Key(field.Label))
	if field.Path {
		row.ColumnSpan(-1)
	}
	row.Children(func() {
		k.captureSettingsLabel(c, field.Label)
		if field.Path {
			ui.Column(c).Width(180).MaxWidthPercent(100).Grow(1.17).Shrink(0).Children(func() {
				k.cliSettingText(c, field.Value).FillWidth().Tooltip(field.Value).Selectable()
			})
			return
		}
		k.cliSettingText(c, field.Value)
	})
}

func (k Kit) cliSettingsRow(c *ui.Context) ui.Element {
	return k.SettingsField(c, SettingsFieldOptions{Detail: true}).MinHeight(34)
}

func (k Kit) cliSettingText(c *ui.Context, value string) ui.Element {
	return ui.Text(c, value).FontSize(12).LineHeight(1.35).TextColor(c.Theme().TextMuted).Wrap()
}
