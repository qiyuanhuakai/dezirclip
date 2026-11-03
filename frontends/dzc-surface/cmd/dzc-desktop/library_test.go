package main

import (
	"context"
	_ "embed"
	"encoding/base64"
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

//go:embed testdata/ocr-clipboard.png
var ocrFixture []byte

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
	t.Run("managed favorites", func(t *testing.T) {
		// Windows may return the long path even when TEMP used an 8.3 alias.
		sameFile := func(actual, expected string) bool {
			left, leftErr := os.Stat(actual)
			right, rightErr := os.Stat(expected)
			return leftErr == nil && rightErr == nil && os.SameFile(left, right)
		}
		directory := filepath.Join(profile, "emoji_favorites")
		if err := os.Mkdir(directory, 0700); err != nil {
			t.Fatal(err)
		}
		sticker := filepath.Join(directory, "fav_1.png")
		outside := filepath.Join(profile, "private.png")
		for _, path := range []string{sticker, outside} {
			if err := os.WriteFile(path, ocrFixture, 0600); err != nil {
				t.Fatal(err)
			}
		}
		list, err := library.EmojiFavorites(ctx, epoch, nil)
		if err != nil || len(list.Paths) != 1 || !sameFile(list.Paths[0], sticker) {
			t.Fatalf("managed sticker catalogue: %v %v", list.Paths, err)
		}
		source := filepath.Join(t.TempDir(), "dropped.png")
		if err := os.WriteFile(source, ocrFixture, 0600); err != nil {
			t.Fatal(err)
		}
		imported, err := library.MutateFavorite(ctx, epoch, "import", source)
		if err != nil || !sameFile(filepath.Dir(imported), directory) {
			t.Fatalf("file drop: %q %v", imported, err)
		}
		dataURL := "data:image/png;base64," + base64.StdEncoding.EncodeToString(ocrFixture)
		duplicate, err := library.ImportFavoriteData(ctx, epoch, dataURL)
		if err != nil || duplicate != imported {
			t.Fatalf("browser drop: %q %v", duplicate, err)
		}
		if _, err := library.MutateFavorite(ctx, epoch+1, "remove", imported); err == nil {
			t.Fatal("stale deletion succeeded")
		}
		if _, err := library.MutateFavorite(ctx, epoch, "remove", outside); err == nil {
			t.Fatal("unmanaged deletion succeeded")
		}
		if _, err := library.MutateFavorite(ctx, epoch, "remove", imported); err != nil {
			t.Fatal(err)
		}
		refreshed, err := library.EmojiFavorites(ctx, epoch, nil)
		if err != nil || len(refreshed.Paths) != 1 || !sameFile(refreshed.Paths[0], sticker) {
			t.Fatalf("deleted favorite returned: %v %v", refreshed.Paths, err)
		}
		if _, err := os.Stat(imported); !os.IsNotExist(err) {
			t.Fatalf("favorite persisted: %v", err)
		}
		resource, err := library.EmojiFavorites(ctx, epoch, &sticker)
		if err != nil || resource.Preview == nil || !strings.HasPrefix(*resource.Preview, "data:image/png;base64,") {
			t.Fatalf("managed thumbnail: %v", err)
		}
		if _, err := library.EmojiFavorites(ctx, epoch, &outside); err == nil {
			t.Fatal("unmanaged image was exposed")
		}
	})
	watcher := History{actions: owner}
	defer func() {
		if watcher.events != nil {
			watcher.events.Close()
		}
	}()
	baseline, err := watcher.Changes(ctx, nil)
	if err != nil || baseline.State.ServiceInstanceId != instance || len(baseline.Events) != 0 {
		t.Fatalf("new WebView event baseline: %+v %v", baseline, err)
	}
	t.Run("privacy protection preferences", func(t *testing.T) {
		for _, change := range []struct{ key, value string }{
			{"app.privacy_protection", "false"},
			{"app.privacy_protection_kinds", "phone,email"},
			{"app.privacy_protection_custom_rules", "backup example\napp credential"},
			{"app.privacy_protection", "true"},
		} {
			before := epoch
			if err := library.SetSetting(ctx, change.key, change.value, before); err != nil {
				t.Fatal(err)
			}
			updated, err := library.Status(ctx)
			if err != nil || updated.State.PrivacyEpoch <= before {
				t.Fatalf("policy change did not invalidate old reads: %+v %v", updated.State, err)
			}
			epoch = updated.State.PrivacyEpoch
			prefs, err := library.Preferences(ctx)
			var stored map[string]string
			if err != nil || json.Unmarshal(prefs, &stored) != nil || stored[change.key] != change.value {
				t.Fatalf("privacy setting %s did not persist: %v", change.key, err)
			}
			if _, err := library.PasteQueue(ctx, before); err == nil {
				t.Fatal("stale privacy state still authorized reads")
			}
		}
	})
	if err := library.SetWindowPinned(ctx, true, epoch); err != nil {
		t.Fatal(err)
	}
	if err := library.SetSetting(ctx, "app.capture_files", "false", epoch); err != nil {
		t.Fatal(err)
	}
	if err := library.SetSetting(ctx, "app.sequential_mode", "false", epoch); err != nil {
		t.Fatal(err)
	}
	changes, err := watcher.Changes(ctx, &baseline.State.EventSequence)
	if err != nil || len(changes.Events) == 0 || changes.State.EventSequence <= baseline.State.EventSequence {
		t.Fatalf("missing settings changes after startup: %+v %v", changes, err)
	}
	viewSettings := map[string]string{"app.theme": "retro", "app.color_mode": "dark", "app.language": "en", "app.compact_mode": "true",
		"app.clipboard_item_font_size": "18", "app.clipboard_tag_font_size": "8", "app.surface_opacity": "0"}
	fonts, err := library.SystemFonts(ctx)
	if err != nil {
		t.Fatal(err)
	}
	for _, font := range fonts {
		key := "app.font_main"
		if font.IsMono {
			key = "app.font_mono"
		}
		if viewSettings[key] == "" && font.Family != "" {
			if _, err := os.Stat(font.Path); err != nil {
				t.Fatal(err)
			}
			viewSettings[key] = font.Family
		}
	}
	if viewSettings["app.font_main"] == "" || viewSettings["app.font_mono"] == "" {
		t.Fatal("system font picker is missing proportional or monospace families")
	}
	for key, value := range viewSettings {
		if err := library.SetSetting(ctx, key, value, epoch); err != nil {
			t.Fatalf("set %s: %v", key, err)
		}
	}
	if err := library.SetSetting(ctx, "app.theme", "custom:../outside", epoch); err == nil {
		t.Fatal("invalid custom theme identifier accepted")
	}
	for key, value := range map[string]string{"app.clipboard_item_font_size": "19", "app.clipboard_tag_font_size": "7", "app.surface_opacity": "101"} {
		if err := library.SetSetting(ctx, key, value, epoch); err == nil {
			t.Fatalf("accepted out-of-range %s", key)
		}
	}
	if err := library.SetSetting(ctx, "app.capture_files", "invalid", epoch); err == nil {
		t.Fatal("accepted invalid capture preference")
	}
	preferences, err := library.Preferences(ctx)
	var settings map[string]string
	if err != nil || json.Unmarshal(preferences, &settings) != nil || settings["app.capture_files"] != "false" ||
		settings["app.window_pinned"] != "true" || settings["app.sequential_mode"] != "false" {
		t.Fatalf("desktop preferences: %s %v", preferences, err)
	}
	for key, value := range viewSettings {
		if settings[key] != value {
			t.Fatalf("read back %s = %q, want %q", key, settings[key], value)
		}
	}
	if err := library.SetSetting(ctx, "app.font_main", "", epoch); err != nil {
		t.Fatalf("restore theme default font: %v", err)
	}
	preferences, err = library.Preferences(ctx)
	if err != nil || json.Unmarshal(preferences, &settings) != nil || settings["app.font_main"] != "" {
		t.Fatal("cleared font override was not restored")
	}
	themeDir := t.TempDir()
	input := filepath.Join(themeDir, "fixture.dezirtheme.json")
	output := filepath.Join(themeDir, "saved.dezirtheme.json")
	themeData := `{"format":"dezirclip-theme","version":1,"id":"test-theme","name":"测试主题","base":"retro","css":":root.light-mode{--dc-accent:#156c44}"}`
	if err := os.WriteFile(input, []byte(themeData), 0600); err != nil {
		t.Fatal(err)
	}
	theme, err := library.ImportTheme(ctx, input, epoch)
	if err != nil || theme.Id != "test-theme" || theme.Name != "测试主题" {
		t.Fatalf("theme import: %+v %v", theme, err)
	}
	if _, err := library.ImportTheme(ctx, input, epoch); err == nil {
		t.Fatal("duplicate theme import succeeded")
	}
	if err := library.ExportTheme(ctx, theme.Id, output, epoch); err != nil {
		t.Fatal(err)
	}
	exported, err := os.ReadFile(output)
	var savedTheme protocol.DesktopThemePackage
	if err != nil || json.Unmarshal(exported, &savedTheme) != nil || savedTheme.Css != theme.Css || savedTheme.Name != theme.Name {
		t.Fatalf("theme export changed the theme: %v", err)
	}
	if err := library.SetSetting(ctx, "app.theme", "custom:"+theme.Id, epoch); err != nil {
		t.Fatal(err)
	}
	if err := library.RemoveTheme(ctx, theme.Id, epoch); err != nil {
		t.Fatal(err)
	}
	preferences, err = library.Preferences(ctx)
	if err != nil || json.Unmarshal(preferences, &settings) != nil || settings["app.theme"] != "mica" || settings["app.custom_themes"] != "[]" {
		t.Fatalf("active theme removal did not restore mica: %v", err)
	}
	file := filepath.Join(profile, "backup.json")
	const backup = `{"version":"dezirclip-export-v2","exported_at":1,"entries":[{"id":1,"content_type":"text","content":"bridge-marker","source_app":"fixture","created_at":1,"updated_at":1,"tags":["fixture"]}]}`
	if err := os.WriteFile(file, []byte(backup), 0600); err != nil {
		t.Fatal(err)
	}
	preview, err := library.PreviewBackup(ctx, epoch, file)
	if err != nil || len(preview) != 1 || preview[0].Content != "bridge-marker" {
		t.Fatalf("backup preview: %+v %v", preview, err)
	}
	imported, err := library.ImportBackup(ctx, epoch, file, "merge", nil)
	if err != nil || imported.Imported != 1 || imported.Mode != "merge" {
		t.Fatalf("import: %+v %v", imported, err)
	}
	plain := filepath.Join(t.TempDir(), "clipboard-export.json")
	exportedBackup, err := library.ExportBackup(ctx, epoch, plain, "json", nil)
	if err != nil || exportedBackup.Count != 1 || exportedBackup.Encrypted {
		t.Fatalf("JSON export: %+v %v", exportedBackup, err)
	}
	preview, err = library.PreviewBackup(ctx, epoch, plain)
	if err != nil || len(preview) != 1 || preview[0].Content != "bridge-marker" {
		t.Fatalf("exported preview: %+v %v", preview, err)
	}
	encrypted := filepath.Join(t.TempDir(), "clipboard-export.dzc")
	password, wrong := "mygo backup test passphrase", "incorrect backup test passphrase"
	exportedBackup, err = library.ExportBackup(ctx, epoch, encrypted, "encrypted", &password)
	if err != nil || !exportedBackup.Encrypted || exportedBackup.Count != 1 {
		t.Fatalf("encrypted export: %+v %v", exportedBackup, err)
	}
	if _, err := library.ImportBackup(ctx, epoch, encrypted, "replace", &wrong); err == nil {
		t.Fatal("wrong backup passphrase accepted")
	}
	if page, err := library.Page(ctx, protocol.DesktopPageParams{Limit: 10}, epoch); err != nil || len(page) != 1 {
		t.Fatalf("failed replacement changed history: %d %v", len(page), err)
	}
	imported, err = library.ImportBackup(ctx, epoch, encrypted, "replace", &password)
	if err != nil || imported.Imported != 1 || imported.Mode != "replace" {
		t.Fatalf("encrypted restore: %+v %v", imported, err)
	}
	query := "bridge-marker"
	search := protocol.DesktopPageParams{Query: &query, Limit: 40}
	rows, err := library.Page(ctx, search, epoch)
	if err != nil || len(rows) != 1 || rows[0].Instance != instance {
		t.Fatalf("search: %+v %v", rows, err)
	}
	if err := library.SetPasteQueue(ctx, epoch, []int64{rows[0].Id}); err != nil {
		t.Fatal(err)
	}
	if queue, err := library.PasteQueue(ctx, epoch); err != nil || len(queue) != 1 || queue[0] != rows[0].Id {
		t.Fatalf("queue order: %v %v", queue, err)
	}
	if err := library.SetPasteQueue(ctx, epoch+1, []int64{}); err == nil {
		t.Fatal("stale queue clear succeeded")
	}
	if err := library.SetPasteQueue(ctx, epoch, []int64{}); err != nil {
		t.Fatal(err)
	}
	if queue, err := library.PasteQueue(ctx, epoch); err != nil || len(queue) != 0 {
		t.Fatalf("queue clear: %v %v", queue, err)
	}
	page, err := library.TagPage(ctx, protocol.DesktopTagPageParams{Tag: "fixture", SortBy: "time", Limit: 40, ExpectedEpoch: epoch})
	if err != nil || page.Total != 1 || len(page.Items) != 1 || page.Items[0].Id != rows[0].Id {
		t.Fatalf("tag page: %+v %v", page, err)
	}
	params := protocol.DesktopEntryParams{Id: rows[0].Id, ExpectedEpoch: epoch, ServiceInstanceId: instance}
	action := "ignore_source"
	policy := protocol.DesktopPolicyParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Id: &params.Id, Action: &action}
	if decision, err := library.PolicyDecision(ctx, policy); err != nil || decision.Reason == "" {
		t.Fatalf("policy decision: %+v %v", decision, err)
	}
	changed, err := library.PolicyFeedback(ctx, policy)
	if err != nil || changed.State.PrivacyEpoch <= epoch {
		t.Fatalf("policy feedback: %+v %v", changed, err)
	}
	if _, err := library.PolicyRules(ctx, epoch); err == nil {
		t.Fatal("stale policy rules read succeeded")
	}
	rules, err := library.PolicyRules(ctx, changed.State.PrivacyEpoch)
	if err != nil || len(rules) != 1 || rules[0].Kind != action {
		t.Fatalf("policy rules: %+v %v", rules, err)
	}
	removed, err := library.RemovePolicyRule(ctx, rules[0].Id, changed.State.PrivacyEpoch)
	if err != nil || removed.State.PrivacyEpoch <= changed.State.PrivacyEpoch {
		t.Fatalf("remove policy rule: %+v %v", removed, err)
	}
	epoch = removed.State.PrivacyEpoch
	params.ExpectedEpoch = epoch
	if rules, err := library.PolicyRules(ctx, epoch); err != nil || len(rules) != 0 {
		t.Fatalf("removed policy still listed: %+v %v", rules, err)
	}
	catalog, err := library.ToolCatalog(ctx)
	if err != nil || len(catalog.Transforms) == 0 || len(catalog.ContentKinds) == 0 {
		t.Fatalf("tool catalog: %+v %v", catalog, err)
	}
	kinds, err := library.Classify(ctx, "user@example.com")
	if err != nil || len(kinds) != 1 || kinds[0] != "email" {
		t.Fatalf("classify: %v %v", kinds, err)
	}
	transformed, err := library.Transform(ctx, protocol.ToolTransformParams{Kind: "to_uppercase", Text: "café ß 你好"})
	if err != nil || transformed != "CAFÉ SS 你好" {
		t.Fatalf("transform: %q %v", transformed, err)
	}
	for format, marker := range map[string]string{"svg": "<svg", "png": "data:image/png;base64,iVBOR"} {
		qr, err := library.QR(ctx, protocol.ToolQrParams{Format: format, Content: query})
		if err != nil || !strings.Contains(qr, marker) {
			t.Fatalf("QR %s: %v", format, err)
		}
	}
	engine, err := library.OcrEngine(ctx)
	if err != nil || engine.EngineName == "" {
		t.Fatalf("OCR engine: %+v %v", engine, err)
	}
	ocr := protocol.ManualOcrParams{Id: params.Id, ExpectedEpoch: epoch, ServiceInstanceId: instance}
	ocrStatus, err := library.OcrText(ctx, false, ocr)
	if err != nil || ocrStatus.Status != "pending" || ocrStatus.Text != nil {
		t.Fatalf("OCR status: %+v %v", ocrStatus, err)
	}
	if _, err := library.OcrText(ctx, true, ocr); err == nil {
		t.Fatal("recognized a non-image history entry")
	}
	t.Run("recognize PNG and read stored text", func(t *testing.T) {
		body := "data:image/png;base64," + base64.StdEncoding.EncodeToString(ocrFixture)
		id, err := library.AddManual(ctx, body, protocol.ManualEntryParams{ExpectedEpoch: epoch, ContentType: "image", Tags: []string{}})
		if err != nil {
			t.Fatal(err)
		}
		image := protocol.ManualOcrParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Id: id}
		result, err := library.OcrText(ctx, true, image)
		if !engine.Available {
			var remote *client.RemoteError
			if os.Getenv("DZC_TEST_REQUIRE_OCR") == "1" || !errors.As(err, &remote) || remote.Code != protocol.WireErrorCodeUnsupportedCapability {
				t.Fatalf("OCR engine unavailable: %s, recognition error: %v", engine.EngineName, err)
			}
			t.Logf("%s is unavailable; verified explicit refusal", engine.EngineName)
			return
		}
		if err != nil || result.Status != "done" || result.Text == nil {
			t.Fatalf("real %s recognition: %+v %v", engine.EngineName, result, err)
		}
		if strings.Join(strings.Fields(*result.Text), " ") != "DESKTOP TEST" {
			t.Fatalf("%s recognized the fixture as %q", engine.EngineName, *result.Text)
		}
		stored, err := library.OcrText(ctx, false, image)
		if err != nil || stored.Status != "done" || stored.Text == nil || *stored.Text != *result.Text {
			t.Fatalf("stored OCR differs: %+v %v", stored, err)
		}
		t.Logf("verified %s recognized the fixture pixels and persisted the full text", engine.EngineName)
	})
	fresh, err := owner.DesktopConnection(ctx)
	if err != nil {
		t.Fatal(err)
	}
	history := History{actions: owner, events: fresh}
	paste := protocol.DesktopCopyParams{Id: rows[0].Id, ExpectedEpoch: epoch, ServiceInstanceId: instance, Paste: true}
	// Reproduce a connection loss after the UI had a valid target. Retire
	// its real Service socket and reconnect before the user's first click.
	watcher.targetInvalid.Store(false)
	watcher.events.Close()
	if _, err := watcher.Changes(ctx, &changes.State.EventSequence); err == nil || !watcher.targetInvalid.Load() {
		t.Fatalf("lost subscription retained the paste target: %v", err)
	}
	if _, err := watcher.Changes(ctx, nil); err != nil {
		t.Fatal(err)
	}
	if outcome, err := watcher.CopyEntry(ctx, paste); err != nil || outcome != "target_required" {
		t.Fatalf("first paste after reconnect: %q %v", outcome, err)
	}
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
