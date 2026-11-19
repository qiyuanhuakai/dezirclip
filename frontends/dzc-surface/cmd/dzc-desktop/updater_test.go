package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"
)

type indexResponse struct {
	status int
	body   string
}

func (r indexResponse) RoundTrip(*http.Request) (*http.Response, error) {
	return &http.Response{StatusCode: r.status, Body: io.NopCloser(strings.NewReader(r.body))}, nil
}
func TestDesktopUpdateIndexLimits(t *testing.T) {
	for _, test := range []struct {
		response indexResponse
		want     string
		fails    bool
	}{
		{indexResponse{200, `{"version":"1.0.0"}`}, `{"version":"1.0.0"}`, false},
		{indexResponse{404, "Not Found"}, "", false},
		{indexResponse{403, ""}, "", true}, {indexResponse{200, "null"}, "", true},
		{indexResponse{200, strings.Repeat(" ", 256*1024) + "{}"}, "", true},
	} {
		result, err := fetchUpdateIndex(context.Background(), &http.Client{Transport: test.response}, updateIndexBase+"preview.json")
		if (err != nil) != test.fails || string(result) != test.want {
			t.Fatalf("status %d: got %q, %v", test.response.status, result, err)
		}
	}
	for _, link := range []string{"http://github.com/x", "https://github.com.invalid/x", "https://github.com:8443/x", "https://other@github.com/x"} {
		req, err := http.NewRequest("GET", link, nil)
		if err != nil || updateRedirect(req, nil) == nil {
			t.Fatalf("redirect accepted: %s", link)
		}
	}
	req, _ := http.NewRequest("GET", "https://release-assets.githubusercontent.com/download", nil)
	if updateRedirect(req, nil) != nil || updateRedirect(req, make([]*http.Request, 5)) == nil {
		t.Fatal("invalid redirect depth policy")
	}
}

type updateTransport func(*http.Request) (*http.Response, error)

func (f updateTransport) RoundTrip(req *http.Request) (*http.Response, error) { return f(req) }

// Exercise the production desktop request path with the real sibling CLI and isolated channel indexes.
func TestBundledUpdateChannels(t *testing.T) {
	current := os.Getenv("DZC_TEST_LIBRARY_VERSION")
	if current == "" {
		t.Skip("requires sibling CLI")
	}
	version := "999.0.0-alpha.9"
	cpu := map[string]string{"amd64": "x86_64", "arm64": "aarch64"}[runtime.GOARCH]
	suffix := map[string]string{"windows-x86_64": "x64-setup.exe", "windows-aarch64": "arm64-setup.exe",
		"linux-x86_64": "amd64.AppImage", "linux-aarch64": "aarch64.AppImage"}[runtime.GOOS+"-"+cpu]
	file := "DezirClip_" + version + "_" + suffix
	release := "https://github.com/qiyuanhuakai/dezirclip/releases"
	signature := base64.StdEncoding.EncodeToString([]byte("trusted comment: file:" + file + "\tversion:" + version))
	manifest, err := json.Marshal(map[string]any{"version": version, "platforms": map[string]any{
		runtime.GOOS + "-" + cpu: map[string]string{"url": release + "/download/v" + version + "/" + file, "signature": signature}}})
	if err != nil {
		t.Fatal(err)
	}
	var requests []string
	transport := updateTransport(func(req *http.Request) (*http.Response, error) {
		requests = append(requests, req.URL.String())
		reply := indexResponse{404, ""}
		if req.URL.String() == updateIndexBase+"preview.json" {
			reply = indexResponse{200, string(manifest)}
		}
		return reply.RoundTrip(req)
	})
	raw, err := checkManualUpdate(context.Background(), "preview", &http.Client{Transport: transport})
	if err != nil {
		t.Fatal(err)
	}
	nativeResult, err := parseNativeUpdate(raw, "preview")
	if err != nil || nativeResult.Status != "available" || nativeResult.Available == nil || nativeResult.Available.Version != version {
		t.Fatalf("native update result: %+v, %v", nativeResult, err)
	}
	var result struct {
		Status, CurrentVersion, ChannelVersion string
		Support                                struct {
			CanInstall   bool
			Installation string
		}
		Available           struct{ ReleaseURL string }
		UnpublishedChannels []string
	}
	if err := json.Unmarshal(raw, &result); err != nil {
		t.Fatal(err)
	}
	if !slices.Equal(requests, []string{updateIndexBase + "stable.json", updateIndexBase + "preview.json"}) ||
		result.CurrentVersion != current || result.ChannelVersion != version || result.Status != "available" ||
		result.Support.CanInstall || result.Support.Installation != "portable" || result.Available.ReleaseURL != release+"/tag/v"+version ||
		!slices.Equal(result.UnpublishedChannels, []string{"stable"}) {
		t.Fatalf("unexpected channel result: %s, requested %v", raw, requests)
	}
}

func TestNativeUpdateAttemptUsesPerChannelInterval(t *testing.T) {
	database := filepath.Join(t.TempDir(), "clipboard.db")
	now := time.Unix(1_796_000_000, 0)
	check := func(channel string, automatic bool, when time.Time, want bool) {
		t.Helper()
		got, err := recordNativeUpdateAttempt(database, channel, automatic, when)
		if err != nil || got != want {
			t.Fatalf("%s automatic=%v at %v: got %v, %v", channel, automatic, when, got, err)
		}
	}
	check("stable", true, now, true)
	check("stable", true, now.Add(23*time.Hour), false)
	check("preview", true, now.Add(23*time.Hour), true)
	check("stable", false, now.Add(23*time.Hour), true)
	check("stable", true, now.Add(24*time.Hour), false)
	check("stable", true, now.Add(47*time.Hour), true)
	if ran, err := recordNativeUpdateAttempt(database, "../invalid", true, now); ran || err == nil {
		t.Fatal("unknown update channel was accepted")
	}
}

func TestNativeUpdateChannelFollowsBuildUntilExplicitlySelected(t *testing.T) {
	for _, tc := range []struct{ version, saved, want string }{
		{"0.2.0-beta.3", "", "preview"},
		{"0.2.0", "", "stable"},
		{"0.2.0+build-7", "", "stable"},
		{"unknown", "", "stable"},
		{"0.2.0-beta.3", "stable", "stable"},
		{"0.2.0", "preview", "preview"},
	} {
		if got := nativeUpdateChannel(tc.version, tc.saved); got != tc.want {
			t.Fatalf("%s with preference %q: got %q, want %q", tc.version, tc.saved, got, tc.want)
		}
	}
}
