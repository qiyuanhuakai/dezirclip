package buildinfo

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestRuntimeRequiresVerifiedCheckoutAndMatchingBuildIdentity(t *testing.T) {
	version, source, state, target, id, patch := Version, SourceRevision, SourceState, Target, BuildID, ToolkitPatch
	t.Cleanup(func() {
		Version, SourceRevision, SourceState, Target, BuildID, ToolkitPatch = version, source, state, target, id, patch
	})
	Version, SourceRevision, SourceState, Target = "0.2.0-alpha.5", strings.Repeat("a", 40), "verified_checkout", "x86_64-pc-windows-msvc"
	BuildID = Version + "@" + SourceRevision
	ToolkitPatch = RequiredToolkitPatch
	if !Valid() {
		t.Fatal("verified build rejected")
	}
	for _, invalid := range []string{"clean", "modified_checkout", "unknown", ""} {
		SourceState = invalid
		if Valid() {
			t.Fatalf("accepted source state %q", invalid)
		}
	}
	SourceState = "verified_checkout"
	for _, invalid := range []string{"unknown", "", "unpatched", "other-patch"} {
		ToolkitPatch = invalid
		if Valid() {
			t.Fatalf("accepted toolkit patch %q", invalid)
		}
	}
	ToolkitPatch = RequiredToolkitPatch
	BuildID = "different-build"
	if Valid() {
		t.Fatal("mismatched build ID accepted")
	}
}

func TestReportDescribesOnlyThisSurfaceAndItsFixedToolkit(t *testing.T) {
	raw, err := json.Marshal(Report())
	if err != nil {
		t.Fatal(err)
	}
	var report map[string]any
	if json.Unmarshal(raw, &report) != nil {
		t.Fatal("invalid build report")
	}
	if report["component"] != "surface" || report["mygo_version"] != "v0.6.4" || report["mygo_source_revision"] != MyGoSource {
		t.Fatal("untruthful toolkit identity")
	}
	if report["toolkit_patch_id"] != ToolkitPatch {
		t.Fatal("toolkit patch identity differs")
	}
	if _, exists := report["linked_versions"]; exists {
		t.Fatal("Go helper claimed Rust linkage")
	}
}
