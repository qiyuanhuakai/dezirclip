package main

import (
	"os/exec"
	"slices"
	"testing"
)

func TestSoftwareWebviewUsesOnlyTheApplicationSwitch(t *testing.T) {
	t.Setenv("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--example-external-flag")
	child := exec.Command("dzc-desktop.exe", "--main-window", "fixture-instance")
	configureSoftwareWebview(child)
	want := []string{"dzc-desktop.exe", "--main-window", "fixture-instance", "--software-webview"}
	if !slices.Equal(child.Args, want) || child.Env != nil {
		t.Fatal("software mode must leave WebView2 environment policy to the runtime")
	}
}
