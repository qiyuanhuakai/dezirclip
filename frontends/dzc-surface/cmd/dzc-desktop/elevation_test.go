package main

import (
	"encoding/json"
	"os"
	"os/exec"
	"testing"
)

func TestElevationProfilePreservesUserOverrides(t *testing.T) {
	keys := []string{"DEZIRCLIP_DB_PATH", "TIEZ_DB_PATH"}
	for _, key := range keys {
		t.Setenv(key, os.Getenv(key))
	}
	for _, values := range [][]*string{{nil, nil}, {stringPointer("relative/profile.db"), nil}, {nil, stringPointer("C:/数据 profile/clipboard.db")}, {stringPointer(""), stringPointer("legacy.db")}} {
		for i, key := range keys {
			if values[i] == nil {
				os.Unsetenv(key)
			} else {
				os.Setenv(key, *values[i])
			}
		}
		encoded, err := encodeElevationProfile()
		if err != nil {
			t.Fatal(err)
		}
		for _, key := range keys {
			os.Setenv(key, "inherited-helper-value")
		}
		if err := applyElevationProfile(encoded); err != nil {
			t.Fatal(err)
		}
		cmd := exec.Command(os.Args[0], "-test.run=^TestElevationProfileChild$")
		cmd.Env = append(os.Environ(), "DZC_TEST_ELEVATION_PROFILE_CHILD=1")
		output, err := cmd.Output()
		if err != nil {
			t.Fatal(err)
		}
		var inherited map[string]*string
		if err := json.Unmarshal(output, &inherited); err != nil {
			t.Fatalf("%s: %v", output, err)
		}
		for i, key := range keys {
			actual := inherited[key]
			if (actual == nil) != (values[i] == nil) || (actual != nil && *actual != *values[i]) {
				t.Fatalf("child changed override policy for %s", key)
			}
		}
	}
}

func stringPointer(value string) *string { return &value }

func TestElevationProfileChild(t *testing.T) {
	if os.Getenv("DZC_TEST_ELEVATION_PROFILE_CHILD") != "1" {
		return
	}
	values := map[string]*string{}
	for _, key := range []string{"DEZIRCLIP_DB_PATH", "TIEZ_DB_PATH"} {
		if value, ok := os.LookupEnv(key); ok {
			values[key] = &value
		}
	}
	if err := json.NewEncoder(os.Stdout).Encode(values); err != nil {
		os.Exit(1)
	}
	os.Exit(0)
}

func TestIncompleteElevationArgumentsCannotStartHandoff(t *testing.T) {
	previous := os.Args
	t.Cleanup(func() { os.Args = previous })
	for _, mode := range []string{"--elevate-helper", "--after-elevation"} {
		os.Args = []string{previous[0], mode}
		handled, err := prepareElevation()
		if !handled || err == nil {
			t.Fatalf("incomplete %s was not rejected", mode)
		}
	}
	os.Args = []string{previous[0], "--show"}
	if handled, err := prepareElevation(); handled || err != nil {
		t.Fatalf("ordinary launch changed: %v", err)
	}
}
