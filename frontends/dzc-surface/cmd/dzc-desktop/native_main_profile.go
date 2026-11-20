package main

import (
	"context"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type nativeProfileSettings struct {
	directory              string
	schema                 uint32
	failed, reading        bool
	moving, restartPending bool
}

func (n *nativeMain) profileSettings(c *ui.Context) {
	profile := &n.profile
	ready := n.session != nil && n.history.state.ServiceInstanceId != ""
	if ready && profile.directory == "" && !profile.reading && !profile.failed {
		n.loadProfileSettings()
	}
	path := profileDisplayPath(profile.directory)
	if profile.reading {
		path = n.text("common_loading")
	} else if profile.failed {
		path = n.text("native_profile_read_failed")
	}
	kit := n.mainKit()
	kit.SettingsDataPath(c, n.text("data_path"), path, func() {
		if profile.restartPending {
			if kit.SettingsDataAction(c, n.text("common_retry"), "", profile.moving).Clicked() {
				profile.moving = true
				n.notice = n.text("processing")
				window, language := n.window, n.language()
				go n.restartProfile(window, language)
			}
			return
		}
		if profile.failed {
			if kit.SettingsDataAction(c, n.text("common_retry"), "", !ready || n.mutating).Clicked() {
				n.loadProfileSettings()
			}
		}
		disabled := !ready || n.mutating || profile.directory == ""
		if kit.SettingsDataAction(c, n.text("change_app"), "", disabled || n.private).Clicked() {
			n.chooseProfileDirectory()
		}
		if kit.SettingsDataAction(c, n.text("open_folder"), "", disabled).Clicked() {
			n.openProfileDirectory()
		}
	})
}

func profileDisplayPath(path string) string {
	if len(path) > 8 && strings.EqualFold(path[:8], "\\\\?\\UNC\\") {
		return "\\\\" + path[8:]
	}
	if len(path) > 6 && strings.HasPrefix(path, "\\\\?\\") && path[5:7] == ":\\" {
		return path[4:]
	}
	return path
}

func (n *nativeMain) loadProfileSettings() {
	if n.session == nil || n.profile.reading {
		return
	}
	n.profile.reading, n.profile.failed = true, false
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		known, err := n.library.ProfileInfo(ctx)
		n.update(session, func() {
			n.profile.reading = false
			n.profile.schema = known.DatabaseSchemaVersion
			n.profile.directory, n.profile.failed = known.DataDirectory, err != nil || known.DataDirectory == ""
		})
	}()
}

func (n *nativeMain) openProfileDirectory() {
	if n.session == nil || n.mutating {
		return
	}
	session := n.session
	n.mutating, n.notice = true, ""
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		err := n.library.OpenDataFolder(ctx)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = n.text("native_profile_open_failed")
			}
		})
	}()
}

func (n *nativeMain) chooseProfileDirectory() {
	if n.session == nil || n.mutating || n.private || n.window == nil || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating, n.notice = true, ""
	session, window, language := n.session, n.window, n.language()
	state := n.history.state
	n.shell.dock.dialogs.Add(1)
	go func() {
		title := language.text("change_data_path")
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
			Parent: window, Title: title, Directory: true,
		})
		var directory string
		if err == nil && len(paths) != 0 && session.Err() == nil {
			message := strings.ReplaceAll(language.text("data_move_confirm"), "{path}", profileDisplayPath(paths[0]))
			result, confirmError := mygo.Dialog.Message(mygo.MessageOptions{
				Parent: window, Title: title, Message: message, Type: mygo.MessageWarning,
				Buttons:       []string{language.text("common_confirm"), language.text("common_cancel")},
				DefaultButton: 1, CancelButton: 1,
			})
			err = confirmError
			if err == nil && result.Button == 0 {
				directory = paths[0]
			}
		}
		n.shell.dock.dialogs.Add(-1)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = n.text("native_profile_picker_failed")
				return
			}
			if directory == "" || n.private || n.history.state.ServiceInstanceId != state.ServiceInstanceId ||
				n.history.state.PrivacyEpoch != state.PrivacyEpoch {
				return
			}
			n.profile.moving = true
			n.stopSession()
			n.notice = n.text("processing")
			go n.moveProfile(window, language, directory, state.PrivacyEpoch)
		})
	}()
}

func (n *nativeMain) moveProfile(window *mygo.Window, language nativeLanguage, directory string, epoch uint64) {
	// The move stops the old Service. Keep the resident request alive across that cutover.
	reconnect, err := n.shell.MoveProfile(context.Background(), directory, epoch)
	if err != nil {
		message := strings.ReplaceAll(language.text("data_move_failed"), "{e}", err.Error())
		n.profileMessage(window, language, message, mygo.MessageError)
		window.Update(func() {
			n.profile.moving = false
			n.prepare(n.resume)
			n.notice = message
		})
		return
	}
	message := language.text("data_move_success")
	if reconnect {
		message = language.text("data_move_reconnect")
	}
	window.Update(func() { n.profile.directory, n.notice = directory, message })
	n.profileMessage(window, language, message, mygo.MessageInfo)
	n.restartProfile(window, language)
}

func (n *nativeMain) restartProfile(window *mygo.Window, language nativeLanguage) {
	if err := n.shell.RelaunchProfile(context.Background()); err != nil {
		message := language.text("data_move_retry")
		n.profileMessage(window, language, message, mygo.MessageWarning)
		window.Update(func() {
			n.profile.moving, n.profile.restartPending = false, true
			n.notice = message
		})
	}
}

func (n *nativeMain) profileMessage(window *mygo.Window, language nativeLanguage, message string, kind mygo.MessageType) {
	if window.IsDestroyed() || !window.IsVisible() {
		return
	}
	n.shell.dock.dialogs.Add(1)
	defer n.shell.dock.dialogs.Add(-1)
	_, _ = mygo.Dialog.Message(mygo.MessageOptions{
		Parent: window, Title: language.text("notice"), Message: message, Type: kind,
		Buttons: []string{language.text("common_confirm", "OK")},
	})
}
