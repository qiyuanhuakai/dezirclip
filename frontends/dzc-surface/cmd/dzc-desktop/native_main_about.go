package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeUpdater struct {
	channel              string
	automatic, notesOpen bool
	confirm, installing  bool
	timer                *time.Timer
	cancel               context.CancelFunc
	generation           uint64
	result               nativeUpdateResult
	err                  error
	checked              time.Time
}

type nativeUpdateResult struct {
	Channel, CurrentVersion, ChannelVersion, Status string
	Available                                       *struct {
		Version, Notes, ReleaseURL string
		Artifact                   *mygoArtifact
	}
	UnpublishedChannels           []string
	installTarget, installSupport string
}

var (
	preReleaseVersion = regexp.MustCompile(`^\d+\.\d+\.\d+-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*(?:\+[0-9A-Za-z.-]+)?$`)
	errUpdateSchedule = errors.New("native_update_schedule_unavailable")
	errUpdateLink     = errors.New("native_update_link_unavailable")
)

func nativeUpdateChannel(version, saved string) string {
	if saved == "stable" || saved == "preview" {
		return saved
	}
	if preReleaseVersion.MatchString(version) {
		return "preview"
	}
	return "stable"
}

func (n *nativeMain) configureUpdater(saved string, automatic bool) {
	u := &n.updater
	channel := nativeUpdateChannel(buildinfo.Version, saved)
	if u.channel == channel && u.automatic == automatic {
		return
	}
	n.stopUpdater()
	if channel != u.channel {
		u.result, u.err, u.checked, u.notesOpen = nativeUpdateResult{}, nil, time.Time{}, false
		u.result.installTarget, u.result.installSupport = mygoUpdateTarget()
	}
	u.channel, u.automatic = channel, automatic
	if automatic {
		generation := u.generation
		u.timer = time.AfterFunc(15*time.Second, func() {
			n.window.Update(func() {
				if !n.window.IsDestroyed() && n.updater.generation == generation {
					n.checkForUpdates(true)
				}
			})
		})
	}
}

func (n *nativeMain) stopUpdater() {
	u := &n.updater
	u.generation++
	u.confirm, u.installing = false, false
	if u.timer != nil {
		u.timer.Stop()
		u.timer = nil
	}
	if u.cancel != nil {
		u.cancel()
		u.cancel = nil
	}
}

func (n *nativeMain) checkForUpdates(automatic bool) {
	u := &n.updater
	if u.channel == "" || u.cancel != nil || automatic && !u.automatic {
		return
	}
	n.stopUpdater()
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Second)
	u.cancel = cancel
	channel, generation := u.channel, u.generation
	go func() {
		defer cancel()
		result, attempted, err := n.fetchUpdate(ctx, channel, automatic)
		n.window.Update(func() {
			if n.window.IsDestroyed() || n.updater.generation != generation {
				return
			}
			u.cancel = nil
			if err != nil {
				u.err = err
			} else if attempted {
				u.result, u.err, u.checked, u.notesOpen = result, nil, time.Now(), false
			}
		})
	}()
}

func (n *nativeMain) fetchUpdate(ctx context.Context, channel string, automatic bool) (nativeUpdateResult, bool, error) {
	var result nativeUpdateResult
	database, err := n.library.owner.DesktopDatabase()
	due := true
	if err == nil {
		due, err = recordNativeUpdateAttempt(database, channel, automatic, time.Now())
	}
	if automatic && (err != nil || !due) {
		if err != nil {
			err = errUpdateSchedule
		}
		return result, false, err
	}
	raw, err := n.shell.CheckForUpdate(ctx, channel)
	if err == nil {
		result, err = parseNativeUpdate(raw, channel)
		result.installTarget, result.installSupport = mygoUpdateTarget()
	}
	return result, true, err
}

func (n *nativeMain) installUpdate() {
	u := &n.updater
	available := u.result.Available
	if u.cancel != nil || available == nil || available.Artifact == nil || u.result.installTarget == "" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 12*time.Minute)
	u.cancel, u.installing = cancel, true
	u.err = nil
	generation, target := u.generation, u.result.installTarget
	go func() {
		defer cancel()
		version, artifact := available.Version, *available.Artifact
		path, err := downloadMyGoUpdate(ctx, artifact, version, target)
		if err == nil {
			request := mygoInstallRequest{Path: path, Version: version, Signature: artifact.Signature}
			err = n.shell.resident.send(ctx, residentRequest{Operation: "install-update", Update: &request}, 30*time.Second)
		}
		n.window.Update(func() {
			if n.window.IsDestroyed() || u.generation != generation {
				return
			}
			u.cancel, u.installing, u.err = nil, false, err
		})
	}()
}

// Record external requests before they run so a restarted main window preserves the interval.
func recordNativeUpdateAttempt(database, channel string, automatic bool, now time.Time) (bool, error) {
	if (channel != "stable" && channel != "preview") || !filepath.IsAbs(database) {
		return false, client.ErrProtocol
	}
	directory := filepath.Dir(database)
	path := filepath.Join(directory, "mygo-update-"+channel+".stamp")
	if automatic {
		info, err := os.Lstat(path)
		switch {
		case err == nil && !info.Mode().IsRegular():
			return false, errUpdateSchedule
		case err == nil && now.Sub(info.ModTime()) < 24*time.Hour:
			return false, nil
		case err != nil && !os.IsNotExist(err):
			return false, err
		}
	}
	file, err := os.CreateTemp(directory, ".mygo-update-")
	if err != nil {
		return false, err
	}
	defer os.Remove(file.Name())
	if err := file.Close(); err != nil {
		return false, err
	}
	if err := os.Chtimes(file.Name(), now, now); err != nil {
		return false, err
	}
	if err := os.Rename(file.Name(), path); err != nil {
		return false, err
	}
	return true, nil
}

func parseNativeUpdate(raw json.RawMessage, channel string) (nativeUpdateResult, error) {
	var result nativeUpdateResult
	if err := json.Unmarshal(raw, &result); err != nil || result.Channel != channel || result.CurrentVersion == "" {
		return nativeUpdateResult{}, client.ErrProtocol
	}
	switch result.Status {
	case "available":
		if result.Available != nil && result.Available.Version != "" && result.Available.ReleaseURL != "" {
			return result, nil
		}
	case "current", "ahead", "unpublished":
		if result.Available == nil {
			return result, nil
		}
	}
	return nativeUpdateResult{}, client.ErrProtocol
}

func (n *nativeMain) aboutSettings(c *ui.Context, kit uikit.Kit) {
	ui.Text(c, n.text("build_info")).FontWeight(600)
	kit.SettingsDisclosure(c, n.text("build_details"), &n.buildDetailsOpen, func() {
		ui.Column(c).Gap(6).FillWidth().Padding(6).Children(func() { n.aboutBuildDetails(c) })
	})
	ui.Text(c, n.text("updater_title")).FontWeight(600)
	n.updateSettings(c, kit)
}

func (n *nativeMain) aboutBuildDetails(c *ui.Context) {
	version := protocol.Version
	source := n.text("build_source_unknown")
	switch buildinfo.SourceState {
	case "verified_checkout":
		source = n.text("build_source_verified")
	case "modified_checkout":
		source = n.text("build_source_modified")
	}
	for _, pair := range [][2]string{
		{n.text("app_version"), buildinfo.Version},
		{n.text("build_identifier"), buildinfo.BuildID},
		{n.text("build_source_revision"), buildinfo.SourceRevision},
		{n.text("build_source_state"), source},
		{n.text("build_target"), buildinfo.Target},
		{n.text("build_protocol"), fmt.Sprintf("%d.%d", version.Major, version.Minor)},
		{"MyGo", buildinfo.MyGoVersion()},
	} {
		n.aboutValue(c, pair[0], pair[1])
	}
	switch {
	case n.profile.schema != 0:
		n.aboutValue(c, n.text("build_database_schema"), strconv.FormatUint(uint64(n.profile.schema), 10))
	case n.profile.failed:
		ui.Text(c, n.text("build_info_unavailable")).Wrap()
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("build_info_retry"), Disabled: n.session == nil}).Clicked() {
			n.loadProfileSettings()
		}
	default:
		ui.Text(c, n.text("build_info_loading")).Wrap()
		n.loadProfileSettings()
	}
}

func (n *nativeMain) aboutValue(c *ui.Context, label, value string) {
	ui.Text(c, label).FontWeight(600).Wrap()
	ui.Text(c, value).Wrap().Selectable()
}

func (n *nativeMain) updateSettings(c *ui.Context, kit uikit.Kit) {
	u := &n.updater
	busy := u.cancel != nil
	disabled := u.channel == "" || busy || n.mutating || n.session == nil || n.history.state.ServiceInstanceId == ""
	auto := u.automatic
	if kit.SettingsToggle(c, &auto, n.text("updater_auto_check")).Disabled(disabled).Changed() {
		n.saveSetting("app.update_auto_check", strconv.FormatBool(auto))
	}
	channel, channelLabel := u.channel, n.text("updater_channel")
	ui.Text(c, channelLabel).Wrap()
	if kit.Select(c, &channel, channelLabel, []uikit.SelectOption{
		{Value: "stable", Label: n.text("updater_channel_stable_name")},
		{Value: "preview", Label: n.text("updater_channel_preview_name")},
	}, disabled).Changed() {
		n.saveSetting("app.update_channel", channel)
	}
	current := buildinfo.Version
	if u.result.CurrentVersion != "" {
		current = u.result.CurrentVersion
	}
	n.aboutValue(c, n.text("updater_current_version"), current)
	if u.result.ChannelVersion != "" {
		n.aboutValue(c, n.text("updater_channel_version"), u.result.ChannelVersion)
	}
	check := n.text("updater_check_now")
	if busy {
		check = n.text("updater_checking")
		if u.installing {
			check = n.text("updater_downloading")
		}
	}
	ui.Row(c).Gap(6).Wrap().Children(func() {
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: check, Disabled: disabled}).Clicked() {
			n.checkForUpdates(false)
		}
		label, target := n.text("updater_all_releases"), desktopReleasesURL
		if available := u.result.Available; available != nil {
			label, target = n.text("updater_open_release"), available.ReleaseURL
			if available.Artifact != nil && u.result.installTarget != "" &&
				n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("updater_install"), Disabled: disabled}).Clicked() {
				u.confirm = true
			}
		}
		if n.mainKit().Button(c, uikit.ButtonOptions{Label: label}).Clicked() {
			if err := n.shell.OpenURL(target); err != nil {
				u.err = errUpdateLink
			} else if errors.Is(u.err, errUpdateLink) {
				u.err = nil
			}
		}
	})
	if u.result.installSupport != "" {
		support := n.text("updater_support_unavailable")
		switch u.result.installSupport {
		case "updater_support_nsis":
			support = n.text("updater_support_nsis")
		case "updater_support_appimage":
			support = n.text("updater_support_appimage")
		case "updater_support_readonly":
			support = n.text("updater_support_readonly")
		case "updater_support_package":
			support = n.text("updater_support_package")
		case "updater_support_portable":
			support = n.text("updater_support_portable")
		}
		ui.Text(c, support).Wrap()
	}
	status := n.text("updater_not_checked")
	switch u.result.Status {
	case "current":
		status = n.text("updater_current")
	case "ahead":
		status = n.text("updater_ahead")
	case "available":
		status = n.text("updater_found")
	case "unpublished":
		status = n.text("updater_unpublished")
	}
	if !busy {
		ui.Text(c, status).Wrap()
	}
	if !u.checked.IsZero() {
		ui.Text(c, n.text("updater_checked_at")+" "+u.checked.Format("15:04:05")).Wrap()
	}
	if !busy && u.err != nil {
		ui.Text(c, n.updateError(u.err)).TextColor(c.Theme().Danger).Wrap()
	}
	if u.channel == "preview" && u.result.Status != "unpublished" && slices.Contains(u.result.UnpublishedChannels, "preview") {
		ui.Text(c, n.text("updater_preview_index_missing")).Wrap()
	}
	if available := u.result.Available; available != nil {
		kit.SettingsDisclosure(c, n.text("updater_release_notes"), &u.notesOpen, func() {
			notes := available.Notes
			if notes == "" {
				notes = n.text("updater_no_notes")
			}
			ui.Text(c, notes).Wrap().Selectable()
		})
	}
}

func (n *nativeMain) updateError(err error) string {
	switch err.Error() {
	case "update_download_or_signature_failed":
		return n.text("updater_error_download_verify")
	case "update_busy":
		return n.text("updater_error_busy")
	case errUpdateSchedule.Error():
		return n.text("updater_error_storage")
	case errUpdateLink.Error():
		return n.text("updater_error_link")
	case "update_check_failed":
		return n.text("updater_error_network")
	case "update_server_failed":
		return n.text("updater_error_server")
	case "update_invalid_manifest", "update_invalid_stable_manifest", "update_invalid_version", "update_invalid_endpoint":
		return n.text("updater_error_metadata")
	case "update_untrusted_download", "update_invalid_signature", "update_signature_version_mismatch":
		return n.text("updater_error_verification")
	default:
		return n.text("updater_error_unknown")
	}
}
