package main

import (
	"context"
	"encoding/json"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

type nativeCLIInfo struct {
	Path              string `json:"cli_path"`
	Version           string `json:"cli_version"`
	CLIGuide          string `json:"skill_path"`
	ThemeGuide        string `json:"theme_skill_path"`
	OnPath            bool   `json:"cli_on_path"`
	RegisteredForUser bool   `json:"cli_registered_for_user"`
	PathCLI           string `json:"path_cli_path"`
	PathVersion       string `json:"path_cli_version"`
	PathConflict      bool   `json:"cli_path_conflict"`
}

type nativeCLIPathResult struct {
	PathEntry           string `json:"path_entry"`
	AlreadyLinked       bool   `json:"already_linked"`
	RequiresNewTerminal bool   `json:"requires_new_terminal"`
}

type nativeCLISettings struct {
	info       *nativeCLIInfo
	pending    string
	loadFailed bool
	notice     string
}

func (n *nativeMain) cliSettings(c *ui.Context, kit uikit.Kit) {
	state := &n.cli
	if state.info == nil && !state.loadFailed && state.pending == "" {
		n.runCLITool("info")
	}
	var info nativeCLIInfo
	if state.info != nil {
		info = *state.info
	}
	status := n.cliStatus(info)
	options := uikit.CLISettingsOptions{
		Title: n.text("dzc_cli"), Description: n.text("dzc_cli_hint"), HelpLabel: n.text("settings_help"),
		Status: status, Notice: state.notice,
		GuidesTitle: n.text("agent_skill"), GuidesDescription: n.text("agent_skill_hint"), GuideOpenLabel: n.text("open_skill_folder"),
		Guides: []uikit.CLISettingGuide{
			{Label: n.text("skill_cli"), Path: profileDisplayPath(info.CLIGuide), Fallback: "dzc-cli", Disabled: state.pending != "",
				Open: func() { n.runCLITool("open-cli-guide") }},
			{Label: n.text("skill_theme"), Path: profileDisplayPath(info.ThemeGuide), Fallback: "dezirclip-theme", Disabled: state.pending != "",
				Open: func() { n.runCLITool("open-theme-guide") }},
		},
	}
	if info.Path != "" {
		options.Fields = append(options.Fields, uikit.CLISettingField{Label: n.text("cli_path"), Value: profileDisplayPath(info.Path), Path: true})
	}
	if info.Version != "" {
		options.Fields = append(options.Fields, uikit.CLISettingField{Label: n.text("cli_version"), Value: info.Version})
	}
	if info.PathConflict && info.PathCLI != "" {
		options.Conflict = append(options.Conflict, uikit.CLISettingField{Label: n.text("cli_path_active_path"), Value: profileDisplayPath(info.PathCLI), Path: true})
		if info.PathVersion != "" {
			options.Conflict = append(options.Conflict, uikit.CLISettingField{Label: n.text("cli_path_active_version"), Value: info.PathVersion})
		}
	}
	kit.CLISettings(c, options)
}

func (n *nativeMain) cliStatus(info nativeCLIInfo) uikit.CLISettingsStatus {
	state := &n.cli
	status := uikit.CLISettingsStatus{Label: n.text("cli_path_status"), Disabled: state.pending != ""}
	switch {
	case state.pending == "info":
		status.Value = n.text("processing")
	case state.loadFailed:
		status.Value = n.text("cli_info_load_failed")
		status.Action, status.Run = n.text("common_retry"), func() { n.runCLITool("info") }
	case info.PathConflict:
		status.Value = n.text("cli_path_other_copy_badge")
	case info.OnPath:
		status.Value, status.Available = n.text("cli_path_ready_badge"), true
	case info.RegisteredForUser:
		status.Value = n.text("cli_path_registered_badge")
	case info.Path != "":
		status.Value = n.text("cli_path_missing_badge")
		status.Action, status.Run = n.text("add_to_path"), func() { n.runCLITool("add-to-path") }
		if state.pending == "add-to-path" {
			status.Action = n.text("processing")
		}
	default:
		status.Value = n.text("cli_path_not_found_badge")
	}
	return status
}

func (n *nativeMain) runCLITool(action string) {
	window := n.window
	if window == nil || n.cli.pending != "" {
		return
	}
	n.cli.pending = action
	if action == "add-to-path" {
		n.cli.notice = ""
	}
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		var result *nativeCLIPathResult
		var info *nativeCLIInfo
		var actionErr, infoErr error
		if action != "info" {
			raw, err := client.PackageTool(ctx, action)
			actionErr = err
			if err == nil && action == "add-to-path" {
				actionErr = json.Unmarshal(raw, &result)
				if result == nil && actionErr == nil {
					actionErr = client.ErrProtocol
				}
			}
		}
		readInfo := action == "info" || action == "add-to-path" && actionErr == nil
		if readInfo {
			info, infoErr = readNativeCLIInfo(ctx)
		}
		window.Update(func() {
			if window.IsDestroyed() {
				return
			}
			n.cli.pending = ""
			if readInfo {
				n.cli.loadFailed = infoErr != nil
				if !n.cli.loadFailed {
					n.cli.info = info
				}
			}
			n.finishCLITool(action, result, actionErr)
		})
	}()
}

func readNativeCLIInfo(ctx context.Context) (*nativeCLIInfo, error) {
	raw, err := client.PackageTool(ctx, "info")
	if err != nil {
		return nil, err
	}
	var info *nativeCLIInfo
	if err := json.Unmarshal(raw, &info); err != nil {
		return nil, err
	}
	if info == nil {
		return nil, client.ErrProtocol
	}
	return info, nil
}

func (n *nativeMain) finishCLITool(action string, result *nativeCLIPathResult, err error) {
	switch {
	case action == "add-to-path" && err != nil:
		n.cli.notice = n.text("cli_path_add_failed") + " " + err.Error()
	case action == "add-to-path":
		message := n.text("cli_path_added")
		if result.AlreadyLinked {
			message = n.text("cli_path_already_linked")
		} else if result.RequiresNewTerminal {
			message = n.text("cli_path_added_restart")
		}
		n.cli.notice = message + " " + profileDisplayPath(result.PathEntry)
	case err != nil:
		n.cli.notice = n.text("open_skill_folder_failed") + " " + err.Error()
	}
}
