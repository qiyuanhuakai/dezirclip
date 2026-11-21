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
	channel                string
	automatic, notesOpen   bool
	confirm, installFailed bool
	startingInstall        bool
	checking               bool
	generation             uint64
	result                 nativeUpdateResult
	err                    error
	checked                time.Time
	install                nativeInstallProgress
}

type nativeInstallPhase uint8

const (
	updateNotInstalling nativeInstallPhase = iota
	updateDownloading
	updateVerified
	updateInstalling
)

type nativeInstallProgress struct {
	Phase             nativeInstallPhase
	Downloaded, Total int64
}

type residentUpdateStatus struct {
	Progress nativeInstallProgress
	Failure  string
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

type nativeUpdateCheck struct {
	Result  nativeUpdateResult
	Checked time.Time
}

const nativeUpdateInterval = 24 * time.Hour

func (u *nativeUpdater) keepWindow() bool {
	return u.startingInstall
}

func (u *nativeUpdater) installing() bool {
	return u.startingInstall || u.install.Phase != updateNotInstalling
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
		n.restoreUpdateCheck(channel, u.generation)
	}
	u.channel, u.automatic = channel, automatic
}

func (n *nativeMain) restoreUpdateCheck(channel string, generation uint64) {
	if n.window == nil || n.shell == nil {
		return
	}
	go func() {
		var response residentReply
		request := residentRequest{Operation: "update-check", UpdateChannel: channel}
		if n.shell.resident.exchange(context.Background(), request, 4*time.Second, &response) != nil {
			return
		}
		check := response.UpdateCheck
		if !check.validFor(channel, buildinfo.Version, time.Now()) {
			return
		}
		n.window.Update(func() {
			u := &n.updater
			if n.window.IsDestroyed() || u.generation != generation {
				return
			}
			u.applyCheck(check)
		})
	}()
}

func (check *nativeUpdateCheck) validFor(channel, version string, now time.Time) bool {
	return check != nil && check.Result.Channel == channel && check.Result.CurrentVersion == version &&
		!check.Checked.After(now) && now.Sub(check.Checked) < nativeUpdateInterval
}

func (n *nativeMain) stopUpdater() {
	u := &n.updater
	u.generation++
	u.confirm = false
	u.checking = false
}

func (u *nativeUpdater) applyCheck(check *nativeUpdateCheck) {
	if check.Result.Channel != u.channel || !check.Checked.After(u.checked) {
		return
	}
	result := check.Result
	if u.confirm {
		previous, candidate := u.result.Available, result.Available
		u.confirm = previous != nil && candidate != nil && previous.Version == candidate.Version &&
			previous.Artifact != nil && candidate.Artifact != nil && *previous.Artifact == *candidate.Artifact
	}
	result.installTarget, result.installSupport = u.result.installTarget, u.result.installSupport
	u.result, u.err, u.checked, u.notesOpen = result, nil, check.Checked, false
}

func (n *nativeMain) checkForUpdates() {
	u := &n.updater
	if u.channel == "" || u.checking || u.installing() {
		return
	}
	n.stopUpdater()
	u.checking, u.installFailed = true, false
	request := residentRequest{Operation: "update-check", UpdateChannel: u.channel, UpdateNow: true}
	generation := u.generation
	go func() {
		err := n.shell.resident.send(context.Background(), request, 5*time.Second)
		if err == nil {
			return
		}
		n.window.Update(func() {
			if n.window.IsDestroyed() || n.updater.generation != generation {
				return
			}
			u.checking, u.err = false, err
		})
	}()
}

func (u *nativeUpdater) applyCheckEvent(reply residentReply) {
	if reply.UpdateChannel != u.channel || u.installing() {
		return
	}
	u.checking = false
	if reply.Error != "" {
		u.err = errors.New(reply.Error)
	} else if reply.UpdateCheck != nil {
		u.applyCheck(reply.UpdateCheck)
	}
}

func (n *nativeMain) installUpdate() {
	u := &n.updater
	available := u.result.Available
	if u.checking || u.installing() || available == nil || available.Artifact == nil || u.result.installTarget == "" {
		return
	}
	u.startingInstall, u.installFailed, u.err = true, false, nil
	request := residentRequest{Operation: "install-update", UpdateChannel: u.channel, UpdateVersion: available.Version}
	go func() {
		err := n.shell.resident.send(context.Background(), request, 5*time.Second)
		n.window.Update(func() {
			if n.window.IsDestroyed() {
				return
			}
			u.startingInstall = false
			if err != nil && !u.installing() {
				u.installFailed, u.err = true, err
			}
		})
	}()
}

func (u *nativeUpdater) applyInstallStatus(status residentUpdateStatus) {
	u.startingInstall = false
	u.install = status.Progress
	u.installFailed = status.Failure != ""
	u.err = nil
	if u.installFailed {
		u.err = errors.New(status.Failure)
	}
}

// Record external requests before they run so a restarted desktop preserves the interval.
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
		case err == nil && now.Sub(info.ModTime()) < nativeUpdateInterval:
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
	busy := u.checking || u.installing()
	disabled := u.channel == "" || busy || n.mutating || n.session == nil || n.history.state.ServiceInstanceId == ""
	title := n.text("updater_title")
	support := n.updaterSupportDescription(u.result.installSupport)
	if !busy && u.result.Status == "" && u.err == nil {
		support += "\n" + n.text("updater_not_checked")
	}
	kit.SettingsFieldLabel(c, title, n.settingsHelpLabel(title), support)
	auto := u.automatic
	autoLabel := n.text("updater_auto_check")
	if kit.SettingsToggleField(c, &auto, autoLabel, n.settingsHelpLabel(autoLabel), n.text("updater_auto_hint"), disabled) {
		n.saveSetting("app.update_auto_check", strconv.FormatBool(auto))
	}
	channelLabel := n.text("updater_channel")
	channelHint := n.text("updater_stable_hint")
	if u.channel == "preview" {
		channelHint = n.text("updater_preview_hint")
	}
	kit.SettingsField(c, uikit.SettingsFieldOptions{}).Children(func() {
		kit.SettingsFieldLabel(c, channelLabel, n.settingsHelpLabel(channelLabel), channelHint).Grow(1).MinWidth(110)
		kit.SettingsChoices(c, channelLabel, u.channel, []uikit.SelectOption{
			{Value: "stable", Label: n.text("updater_channel_stable_name")},
			{Value: "preview", Label: n.text("updater_channel_preview_name")},
		}, true, disabled, func(channel string) { n.saveSetting("app.update_channel", channel) })
	})
	current := buildinfo.Version
	if u.result.CurrentVersion != "" {
		current = u.result.CurrentVersion
	}
	kit.UpdateVersionRow(c, n.text("updater_current_version"), current)
	channelVersion := u.result.ChannelVersion
	if channelVersion == "" && u.result.Available != nil {
		channelVersion = u.result.Available.Version
	}
	if channelVersion != "" {
		kit.UpdateVersionRow(c, n.text("updater_channel_version"), channelVersion)
	}
	check := n.text("updater_check_now")
	if u.checking {
		check = n.text("updater_checking")
	}
	kit.UpdateActions(c, func() {
		if kit.UpdateAction(c, uikit.UpdateCheck, check, disabled).Clicked() {
			n.checkForUpdates()
		}
		label, target := n.text("updater_all_releases"), desktopReleasesURL
		if available := u.result.Available; available != nil {
			label, target = n.text("updater_open_release"), available.ReleaseURL
			if available.Artifact != nil && u.result.installTarget != "" &&
				kit.UpdateAction(c, uikit.UpdateInstall, n.text("updater_install"), disabled).Clicked() {
				u.confirm = true
			}
		}
		if kit.UpdateAction(c, uikit.UpdateReleases, label, false).Clicked() {
			if err := n.shell.OpenURL(target); err != nil {
				u.err = errUpdateLink
			} else if errors.Is(u.err, errUpdateLink) {
				u.err = nil
			}
		}
	})
	n.drawUpdateProgress(c, kit)
	status := ""
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
	if !busy && status != "" {
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

func (n *nativeMain) updaterSupportDescription(reason string) string {
	switch reason {
	case "updater_support_nsis":
		return n.text("updater_support_nsis")
	case "updater_support_appimage":
		return n.text("updater_support_appimage")
	case "updater_support_readonly":
		return n.text("updater_support_readonly")
	case "updater_support_package":
		return n.text("updater_support_package")
	case "updater_support_portable":
		return n.text("updater_support_portable")
	default:
		return n.text("updater_support_unavailable")
	}
}

func (n *nativeMain) drawUpdateProgress(c *ui.Context, kit uikit.Kit) {
	progress := n.updater.install
	options := uikit.UpdateProgressOptions{Amount: "100%", Fraction: 1, Label: n.text("updater_download_progress")}
	switch progress.Phase {
	case updateDownloading:
		options.Status = n.text("updater_downloading")
		if progress.Total > 0 {
			options.Fraction = min(1, float64(progress.Downloaded)/float64(progress.Total))
			options.Amount = fmt.Sprintf("%d%%", min(100, progress.Downloaded*100/progress.Total))
		} else {
			options.Fraction = -1
			options.Amount = fmt.Sprintf("%.1f MiB", float64(progress.Downloaded)/(1024*1024))
		}
	case updateVerified, updateInstalling:
		options.Verified = n.text("updater_verified")
		options.Status = options.Verified
		if progress.Phase == updateInstalling {
			options.Status = n.text("updater_installing")
		}
	default:
		return
	}
	kit.UpdateProgress(c, options)
}

func (n *nativeMain) drawBackgroundUpdate(c *ui.Context, kit uikit.Kit) {
	u := &n.updater
	if u.install.Phase == updateNotInstalling && !u.installFailed {
		return
	}
	ui.Column(c).FillWidth().Padding(4, 8).Children(func() {
		n.drawUpdateProgress(c, kit)
		if u.installFailed && u.err != nil {
			ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(6).Role(ui.RoleStatus).Children(func() {
				ui.Text(c, n.updateError(u.err)).Grow(1).Wrap().TextColor(c.Theme().Danger)
				if kit.Button(c, uikit.ButtonOptions{Label: n.text("common_close")}).Clicked() {
					u.installFailed = false
				}
			})
		}
	})
}

func (n *nativeMain) updateError(err error) string {
	switch err.Error() {
	case "update_download_or_signature_failed":
		return n.text("updater_error_download_verify")
	case "update_busy":
		return n.text("updater_error_busy")
	case "update_install_failed":
		return n.text("updater_error_install")
	case "update_installation_unsupported":
		return n.text("updater_support_unavailable")
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
