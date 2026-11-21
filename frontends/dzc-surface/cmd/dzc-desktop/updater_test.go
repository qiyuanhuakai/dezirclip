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

	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
)

type indexResponse struct {
	status int
	body   string
}

func TestResidentInstallRequiresTheCheckedVersionAndChannel(t *testing.T) {
	result, err := parseNativeUpdate(json.RawMessage(`{
		"channel":"preview","currentVersion":"old","status":"available",
		"available":{"version":"2.0.0","releaseURL":"release","artifact":{"url":"package","signature":"signature"}}
	}`), "preview")
	if err != nil {
		t.Fatal(err)
	}
	result.CurrentVersion = buildinfo.Version
	check := &nativeUpdateCheck{Result: result, Checked: time.Now()}
	artifact, err := residentUpdateArtifact(check, "preview", "2.0.0")
	if err != nil || artifact.URL != "package" {
		t.Fatalf("accepted release was lost: %v", err)
	}
	if _, err := residentUpdateArtifact(check, "stable", "2.0.0"); err == nil {
		t.Fatal("a different channel was allowed to install")
	}
	if _, err := residentUpdateArtifact(check, "preview", "3.0.0"); err == nil {
		t.Fatal("a different version was allowed to install")
	}
	check.Checked = check.Checked.Add(-nativeUpdateInterval)
	if _, err := residentUpdateArtifact(check, "preview", "2.0.0"); err == nil {
		t.Fatal("an expired release check was allowed to install")
	}
}

func TestResidentCheckEventsKeepTheSelectedChannelAndNewestResult(t *testing.T) {
	u := nativeUpdater{channel: "preview", checking: true}
	u.result.installSupport = "updater_support_nsis"
	now := time.Now()
	preview := &nativeUpdateCheck{
		Result:  nativeUpdateResult{Channel: "preview", ChannelVersion: "2.0.0-beta.1"},
		Checked: now,
	}
	u.applyCheckEvent(residentReply{UpdateChannel: "stable", Error: "update_check_failed"})
	if !u.checking || u.err != nil {
		t.Fatal("a different channel interrupted the pending check")
	}
	u.applyCheckEvent(residentReply{UpdateChannel: "preview", UpdateCheck: preview})
	if u.checking || u.result.ChannelVersion != "2.0.0-beta.1" || u.result.installSupport != "updater_support_nsis" {
		t.Fatalf("unexpected resident update result: %+v", u.result)
	}
	older := *preview
	older.Result.ChannelVersion = "1.9.0"
	older.Checked = now.Add(-time.Minute)
	u.applyCheck(&older)
	if u.result.ChannelVersion != "2.0.0-beta.1" {
		t.Fatal("an older result returned by the status request replaced a newer event")
	}
}

func TestReopenedUpdaterRecoversAnActiveCheckAndItsFailure(t *testing.T) {
	now := time.Now()
	failed := failedResidentUpdateCheck("preview", buildinfo.Version, errUpdateVerification, now)
	reopened := nativeUpdater{channel: "preview"}
	reopened.applyCheckEvent(residentReply{ID: 8, Success: true, UpdateChannel: "preview", UpdateChecking: true})
	if !reopened.checking {
		t.Fatal("the reopened window lost the active check")
	}
	payload, err := json.Marshal(residentReply{ID: 9, Success: true, UpdateChannel: "preview", UpdateCheck: failed})
	if err != nil {
		t.Fatal(err)
	}
	bridge := residentBridge{}
	reopened.applyCheckEvent(bridge.accept(string(payload)))
	if reopened.checking || reopened.err == nil || reopened.err.Error() != errUpdateVerification.Error() || !reopened.checked.Equal(now) {
		t.Fatalf("the check failure did not survive reopening: %+v", reopened)
	}
	if !failed.validFor("preview", buildinfo.Version, now) {
		t.Fatal("the failure was not retained as a fresh check result")
	}
	if _, err := residentUpdateArtifact(failed, "preview", "2.0.0"); err == nil {
		t.Fatal("a failed check allowed installation")
	}
	reopened.applyCheck(&nativeUpdateCheck{
		Result: nativeUpdateResult{Channel: "preview", Status: "current"}, Checked: now.Add(-time.Second),
	})
	if reopened.err == nil {
		t.Fatal("an older successful result cleared the recent check error")
	}
	reopened.applyCheckEvent(residentReply{UpdateChannel: "preview", UpdateCheck: &nativeUpdateCheck{
		Result: nativeUpdateResult{Channel: "preview", Status: "current"}, Checked: now.Add(time.Second),
	}})
	if reopened.err != nil || reopened.result.Status != "current" {
		t.Fatal("a newer completed check did not clear the error")
	}
}

func TestFailedNativeInstallDiscardsTheCandidateWithoutLosingReleaseNotes(t *testing.T) {
	u := nativeUpdater{channel: "preview"}
	if err := json.Unmarshal([]byte(`{"available":{"version":"2.0.0","notes":"Release notes","artifact":{"url":"package","signature":"signature"}}}`), &u.result); err != nil {
		t.Fatal(err)
	}
	u.confirm = true
	u.applyInstallStatus(residentUpdateStatus{Failure: "update_install_failed"})
	u.installErr = nil
	if u.confirm || u.result.Available == nil || u.result.Available.Artifact != nil || u.result.Available.Notes != "Release notes" {
		t.Fatalf("the failed install left an actionable candidate or lost its notes: %+v", u.result.Available)
	}
}

func TestUpdateConfirmationKeepsOnlyTheSelectedCandidate(t *testing.T) {
	for _, test := range []struct {
		name, candidate string
		keep            bool
	}{
		{"same release", `{"version":"2.0.0","artifact":{"url":"package","signature":"first"}}`, true},
		{"new version", `{"version":"2.0.1","artifact":{"url":"package","signature":"first"}}`, false},
		{"new package signature", `{"version":"2.0.0","artifact":{"url":"package","signature":"changed"}}`, false},
		{"removed release", `null`, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			now := time.Now()
			u := nativeUpdater{channel: "preview", confirm: true, checked: now}
			if err := json.Unmarshal([]byte(`{"available":{"version":"2.0.0","artifact":{"url":"package","signature":"first"}}}`), &u.result); err != nil {
				t.Fatal(err)
			}
			result := nativeUpdateResult{Channel: "preview"}
			if err := json.Unmarshal([]byte(`{"available":`+test.candidate+`}`), &result); err != nil {
				t.Fatal(err)
			}
			u.applyCheck(&nativeUpdateCheck{Checked: now.Add(time.Second), Result: result})
			if u.confirm != test.keep {
				t.Fatalf("confirmation is %t, want %t", u.confirm, test.keep)
			}
		})
	}
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
