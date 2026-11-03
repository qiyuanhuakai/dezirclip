package main

import (
	"context"
	"encoding/json/v2"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Run the compiled test alongside a matching CLI and Service, with no OS clipboard capture.
func TestReactLibraryWithService(t *testing.T) {
	version := os.Getenv("DZC_TEST_LIBRARY_VERSION")
	if version == "" {
		t.Skip("requires sibling CLI and Service")
	}
	profile := t.TempDir()
	t.Setenv("DEZIRCLIP_DB_PATH", filepath.Join(profile, "clipboard.db"))
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	cli := filepath.Join(filepath.Dir(executable), "dzc")
	if runtime.GOOS == "windows" {
		cli += ".exe"
	}
	if out, err := exec.Command(cli, "service", "start", "--no-capture", "--json").CombinedOutput(); err != nil {
		t.Fatalf("service start: %v %s", err, out)
	}
	t.Cleanup(func() {
		if out, err := exec.Command(cli, "service", "stop").CombinedOutput(); err != nil {
			t.Errorf("service stop: %v %s", err, out)
		}
	})
	ctx := context.Background()
	owner, err := client.OpenDesktop(ctx, version, false)
	if err != nil {
		t.Fatal(err)
	}
	owner.Close()
	library := Library{owner}
	report, err := library.Platform(ctx)
	var platform struct {
		NativeWayland *bool `json:"native_wayland"`
	}
	if err != nil || json.Unmarshal(report, &platform) != nil || platform.NativeWayland == nil {
		t.Fatalf("platform probe: %s %v", report, err)
	}
	status, err := library.Status(ctx)
	if err != nil || status.CaptureOwner || status.Mode != protocol.HostModeService {
		t.Fatalf("isolated status: %+v %v", status, err)
	}
	instance, epoch := status.State.ServiceInstanceId, status.State.PrivacyEpoch
	if err := library.SetWindowPinned(ctx, true, epoch); err != nil {
		t.Fatal(err)
	}
	var updated protocol.OperationResult
	if err := owner.DesktopCall(ctx, protocol.MethodDesktopSettingsCaptureSet, protocol.CaptureSettingParams{
		ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: "app.capture_files", Value: "false",
	}, "OperationResult", &updated); err != nil || !updated.Performed {
		t.Fatalf("capture settings: %+v %v", updated, err)
	}
	preferences, err := library.Preferences(ctx)
	var settings map[string]string
	if err != nil || json.Unmarshal(preferences, &settings) != nil || settings["app.capture_files"] != "false" || settings["app.window_pinned"] != "true" {
		t.Fatalf("desktop preferences: %s %v", preferences, err)
	}
	file := filepath.Join(profile, "backup.json")
	const backup = `{"version":"dezirclip-export-v2","exported_at":1,"entries":[{"id":1,"content_type":"text","content":"bridge-marker","source_app":"fixture","created_at":1,"updated_at":1,"tags":["fixture"]}]}`
	if err := os.WriteFile(file, []byte(backup), 0600); err != nil {
		t.Fatal(err)
	}
	var imported protocol.BackupImportResult
	if err := owner.DesktopCall(ctx, protocol.MethodDesktopBackupImport, protocol.BackupImportParams{
		ServiceInstanceId: instance, ExpectedEpoch: epoch, Path: file, Mode: "merge",
	}, "BackupImportResult", &imported); err != nil || imported.Imported != 1 {
		t.Fatalf("import: %+v %v", imported, err)
	}
	query := "bridge-marker"
	search := protocol.DesktopPageParams{Query: &query, Limit: 40}
	rows, err := library.Page(ctx, search, epoch)
	if err != nil || len(rows) != 1 || rows[0].Instance != instance {
		t.Fatalf("search: %+v %v", rows, err)
	}
	page, err := library.TagPage(ctx, protocol.DesktopTagPageParams{Tag: "fixture", SortBy: "time", Limit: 40, ExpectedEpoch: epoch})
	if err != nil || page.Total != 1 || len(page.Items) != 1 || page.Items[0].Id != rows[0].Id {
		t.Fatalf("tag page: %+v %v", page, err)
	}
	params := protocol.DesktopEntryParams{Id: rows[0].Id, ExpectedEpoch: epoch, ServiceInstanceId: instance}
	fresh, err := owner.DesktopConnection(ctx)
	if err != nil {
		t.Fatal(err)
	}
	history := History{actions: owner, events: fresh}
	paste := protocol.DesktopCopyParams{Id: rows[0].Id, ExpectedEpoch: epoch, ServiceInstanceId: instance, Paste: true}
	_, err = history.CopyEntry(ctx, paste)
	fresh.Close()
	var remote *client.RemoteError
	if !errors.As(err, &remote) || remote.Code != protocol.WireErrorCodeNotFound {
		t.Fatalf("paste without this connection's foreground target: %v", err)
	}
	if outcome, err := history.CopyEntry(ctx, paste); err != nil || outcome != "target_required" {
		t.Fatalf("consumed target must require a new user action: %q %v", outcome, err)
	}
	counts, err := library.TagCounts(ctx, epoch)
	var tags map[string]int
	if err != nil || json.Unmarshal(counts, &tags) != nil || tags["fixture"] != 1 {
		t.Fatalf("tag counts: %s %v", counts, err)
	}
	summary, err := library.Summary(ctx, params)
	if err != nil || summary.Id != rows[0].Id || summary.Instance != instance {
		t.Fatalf("summary: %+v %v", summary, err)
	}
	entry, err := library.Entry(ctx, params)
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(entry)
	var view struct {
		Content  string `json:"content"`
		Instance string `json:"host_instance_id"`
	}
	if err != nil || json.Unmarshal(encoded, &view) != nil || view.Content != query || view.Instance != instance {
		t.Fatalf("React entry projection: %s %v", encoded, err)
	}
	stale := params
	stale.ServiceInstanceId = "other-instance"
	if _, err := library.Entry(ctx, stale); err == nil {
		t.Fatal("accepted another Host instance")
	}
	if err := library.EditTag(ctx, "create", "extra", "", epoch); err != nil {
		t.Fatal(err)
	}
	pinned := true
	mutation := protocol.DesktopMutationParams{Id: params.Id, ServiceInstanceId: instance, ExpectedEpoch: epoch, Action: "pin", Pinned: &pinned}
	if id, err := library.MutateEntry(ctx, mutation); err != nil || id != params.Id {
		t.Fatalf("pin: %d %v", id, err)
	}
	if err := library.ReorderPins(ctx, [][2]int64{{params.Id, 6}}, epoch); err != nil {
		t.Fatal(err)
	}
	newTags := []string{"extra"}
	mutation.Action, mutation.Pinned, mutation.Tags = "tags", nil, &newTags
	if _, err := library.MutateEntry(ctx, mutation); err != nil {
		t.Fatal(err)
	}
	if err := library.EditTag(ctx, "rename", "extra", "renamed", epoch); err != nil {
		t.Fatal(err)
	}
	entry, err = library.Entry(ctx, params)
	if err != nil || !entry.IsPinned || entry.PinnedOrder != 6 || len(entry.Tags) != 1 || entry.Tags[0] != "renamed" {
		t.Fatalf("stored row metadata: %+v %v", entry, err)
	}
	// Clearing history must preserve pinned or tagged rows.
	if err := library.ClearHistory(ctx, epoch); err != nil {
		t.Fatal(err)
	}
	deleted, err := library.DeleteTagEntries(ctx, protocol.LibraryTagBatchParams{Name: "renamed", ExpectedEpoch: epoch,
		Scope: "selected", Ids: []int64{params.Id}})
	if err != nil || deleted != 1 {
		t.Fatalf("delete selected tag entry: %d %v", deleted, err)
	}
	if rows, err := library.Page(ctx, search, epoch); err != nil || len(rows) != 0 {
		t.Fatalf("deleted row remained searchable: %+v %v", rows, err)
	}
	manual := protocol.ManualEntryParams{ContentType: "text", Tags: []string{"manual"}, ExpectedEpoch: epoch, ServiceInstanceId: instance}
	body := strings.Repeat("文字🎋 ", 24000)
	id, err := library.AddManual(ctx, body, manual)
	if err != nil {
		t.Fatal(err)
	}
	manualRow, err := library.Entry(ctx, protocol.DesktopEntryParams{Id: id, ServiceInstanceId: instance, ExpectedEpoch: epoch})
	if err != nil || manualRow.Content != body || manualRow.SourceApp != "Manual" || len(manualRow.Tags) != 1 || manualRow.Tags[0] != "manual" {
		t.Fatalf("chunked manual entry not stored intact: id=%d error=%v", id, err)
	}
	inFlight, err := owner.DesktopConnection(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer inFlight.Close()
	var begun protocol.CaptureBeginResult
	if err := inFlight.Call(ctx, protocol.MethodDesktopHistoryManualBegin, manual, "CaptureBeginResult", &begun); err != nil || begun.CaptureId == nil {
		t.Fatalf("begin manual entry: %v", err)
	}
	after, err := library.SetPrivacy(ctx, protocol.PrivacySetParams{Active: true})
	if err != nil || !after.Privacy.Active || after.State.PrivacyEpoch == epoch {
		t.Fatalf("set privacy: %+v %v", after, err)
	}
	if _, err := library.Entry(ctx, params); err == nil {
		t.Fatal("accepted stale epoch")
	}
	if _, err := library.Page(ctx, search, epoch); err == nil {
		t.Fatal("accepted stale search")
	}
	if err := library.EditTag(ctx, "create", "stale", "", epoch); !errors.As(err, &remote) || remote.Code != protocol.WireErrorCodePrivacyEpochChanged {
		t.Fatalf("stale metadata edit: %v", err)
	}
	var saved protocol.DesktopMutationResult
	if err := inFlight.Call(ctx, protocol.MethodDesktopHistoryManualFinish, protocol.ManualEntryFinish{CaptureId: *begun.CaptureId}, "DesktopMutationResult", &saved); err == nil {
		t.Fatal("manual upload survived a privacy transition")
	}
	manual.ExpectedEpoch = after.State.PrivacyEpoch
	if _, err := library.AddManual(ctx, "private manual note", manual); !errors.As(err, &remote) || remote.Code != protocol.WireErrorCodeManualPersistenceConfirmationRequired {
		t.Fatalf("private save without confirmation: %v", err)
	}
	manual.ConfirmPersistence = true
	if id, err := library.AddManual(ctx, "confirmed manual note", manual); err != nil || id <= 0 {
		t.Fatalf("confirmed manual note: %d %v", id, err)
	}
}
