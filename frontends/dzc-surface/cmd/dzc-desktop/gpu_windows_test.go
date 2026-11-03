package main

import (
	"slices"
	"strings"
	"testing"

	"golang.org/x/sys/windows"
)

func TestSoftwareBrowserFlagsPreserveInheritedArguments(t *testing.T) {
	const key = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"
	t.Setenv(key, `--example="two words" --disable-gpu --disable-features=Other -- positional`)
	flags := func() ([]string, string) {
		t.Helper()
		env, err := softwareWebviewEnvironment()
		if err != nil || len(env) != 1 {
			t.Fatalf("software environment: %v", err)
		}
		line := strings.TrimPrefix(env[0], key+"=")
		args, err := windows.DecomposeCommandLine("webview2.exe " + line)
		if err != nil {
			t.Fatal(err)
		}
		return args[1:], line
	}
	first, line := flags()
	want := []string{"--example=two words", "--disable-gpu", "--disable-features=Other,VizDisplayCompositor",
		"--disable-gpu-compositing", "--disable-software-rasterizer", "--", "positional"}
	if !slices.Equal(first, want) {
		t.Errorf("arguments %q, want %q", first, want)
	}
	t.Setenv(key, line)
	second, _ := flags()
	if !slices.Equal(second, first) {
		t.Fatalf("duplicate software flags %q", second)
	}
}
