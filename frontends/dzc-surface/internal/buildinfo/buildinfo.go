// Package buildinfo reports this Go helper's own build, independently of Rust.
package buildinfo

import (
	_ "embed"
	"encoding/json"

	"github.com/qiyuanhuakai/dezirclip/clients/go/control"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Populated only by the release build command's -X arguments.
var Version = "unknown"
var SourceRevision = "unknown"
var SourceState = "unknown"
var Target = "unknown"
var BuildID = "unknown"
var ToolkitPatch = "unknown"

//go:embed mygo-toolkit.json
var toolkitManifest []byte

var toolkit = loadToolkit()

type toolkitIdentity struct {
	ID      string `json:"id"`
	Version string `json:"version"`
	Source  string `json:"source_revision"`
}

func loadToolkit() toolkitIdentity {
	var identity toolkitIdentity
	if err := json.Unmarshal(toolkitManifest, &identity); err != nil {
		panic(err)
	}
	if identity.ID == "" || identity.Version == "" || identity.Source == "" {
		panic("incomplete MyGo toolkit manifest")
	}
	return identity
}

func MyGoVersion() string { return toolkit.Version }

func Identity() protocol.SurfaceBuildIdentity {
	return protocol.SurfaceBuildIdentity{ComponentVersion: Version, SourceRevision: SourceRevision, BuildId: BuildID, Target: Target}
}

func Valid() bool {
	return SourceState == "verified_checkout" && ToolkitPatch == toolkit.ID && control.SameBuild(Identity(), Identity())
}

func Report() any {
	return componentReport("surface")
}

func DesktopReport() any {
	return componentReport("mygo-desktop")
}

func componentReport(component string) any {
	return struct {
		Component        string                   `json:"component"`
		ComponentVersion string                   `json:"component_version"`
		SourceRevision   string                   `json:"source_revision"`
		SourceState      string                   `json:"source_state"`
		BuildID          string                   `json:"build_id"`
		ProtocolVersion  protocol.ProtocolVersion `json:"protocol_version"`
		MyGoVersion      string                   `json:"mygo_version"`
		MyGoSource       string                   `json:"mygo_source_revision"`
		Target           string                   `json:"target"`
		ToolkitPatchID   string                   `json:"toolkit_patch_id"`
	}{component, Version, SourceRevision, SourceState, BuildID, protocol.Version, toolkit.Version, toolkit.Source, Target, ToolkitPatch}
}
