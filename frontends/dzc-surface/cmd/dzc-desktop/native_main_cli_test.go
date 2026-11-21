package main

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestNativeCLISettingsDistinguishCurrentAndConflictingCopies(t *testing.T) {
	n := nativeMain{theme: "macos", mode: "dark", preferences: map[string]string{"app.language": "en"}}
	n.cli.info = &nativeCLIInfo{
		Path: `C:\Program Files\DezirClip\dzc.exe`, Version: "0.2.0-beta.5",
		CLIGuide: `C:\Program Files\DezirClip\skills\dzc-cli`,
	}
	view := ui.NewTester(func(c *ui.Context) { n.cliSettings(c, n.mainKit()) }, 360, 720)
	assertText := func(key string, exists bool) {
		t.Helper()
		if view.HasText(n.text(key)) != exists {
			t.Fatalf("%s visible: %v; texts: %v", key, !exists, view.Texts())
		}
	}
	assertText("add_to_path", true)
	assertText("cli_path_active_path", false)
	n.cli.info.PathConflict, n.cli.info.OnPath = true, true
	n.cli.info.PathCLI, n.cli.info.PathVersion = `C:\Old Tools\dzc.exe`, "0.1.0"
	view.Frame()
	assertText("cli_path_other_copy_badge", true)
	assertText("cli_path_active_path", true)
	assertText("add_to_path", false)
	if n.cliStatus(*n.cli.info).Available || !view.HasText(n.cli.info.PathCLI) || !view.HasText(n.cli.info.PathVersion) {
		t.Fatalf("a conflicting terminal copy was marked ready or its details were hidden: %v", view.Texts())
	}
	n.cli.loadFailed = true
	view.Frame()
	assertText("cli_info_load_failed", true)
	assertText("common_retry", true)
	assertText("cli_path_not_found_badge", false)
}

func TestNativeCLIToolReportsPathResult(t *testing.T) {
	n := nativeMain{preferences: map[string]string{"app.language": "en"}}
	for _, example := range []struct {
		result nativeCLIPathResult
		key    string
	}{
		{nativeCLIPathResult{PathEntry: "/opt/example"}, "cli_path_added"},
		{nativeCLIPathResult{PathEntry: "/opt/example", AlreadyLinked: true}, "cli_path_already_linked"},
		{nativeCLIPathResult{PathEntry: "/opt/example", RequiresNewTerminal: true}, "cli_path_added_restart"},
	} {
		n.finishCLITool("add-to-path", &example.result, nil)
		if n.cli.notice != n.text(example.key)+" /opt/example" {
			t.Fatalf("wrong PATH result: %s", n.cli.notice)
		}
	}
	n.finishCLITool("add-to-path", nil, errors.New("registry access denied"))
	if n.cli.notice != n.text("cli_path_add_failed")+" registry access denied" {
		t.Fatalf("PATH failure was not shown: %s", n.cli.notice)
	}
}

func TestNativeCLIReadsStagedPackage(t *testing.T) {
	wantCLI := os.Getenv("DZC_TEST_NATIVE_CLI_PACKAGE")
	if wantCLI == "" {
		t.Skip("requires the sibling CLI and both agent guides")
	}
	info, err := readNativeCLIInfo(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	assertPath := func(got, want string) {
		t.Helper()
		actual, err := os.Stat(got)
		expected, wantedErr := os.Stat(want)
		if err != nil || wantedErr != nil || !os.SameFile(actual, expected) {
			t.Fatalf("got %q, expected %q", got, want)
		}
	}
	assertPath(info.Path, wantCLI)
	assertPath(info.PathCLI, filepath.Join(filepath.Dir(os.Getenv("DZC_TEST_NATIVE_CLI_PACKAGE")), "..", "other", filepath.Base(wantCLI)))
	assertPath(info.CLIGuide, filepath.Join(filepath.Dir(wantCLI), "skills", "dzc-cli"))
	assertPath(info.ThemeGuide, filepath.Join(filepath.Dir(wantCLI), "skills", "dezirclip-theme"))
	if !info.PathConflict || !info.OnPath || info.Version == "" || info.Version != info.PathVersion {
		t.Fatalf("conflicting terminal copy was not resolved: %+v", info)
	}
}
