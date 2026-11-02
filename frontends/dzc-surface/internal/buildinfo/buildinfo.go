// Package buildinfo reports this Go helper's own build, independently of Rust.
package buildinfo

import (
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

const MyGoVersion = "v0.6.4"
const MyGoSource = "6255ea40139d06e0762a7c4a32626148ba410478"
const RequiredToolkitPatch = "mygo-v0.6.4-no-startup-cleanup-v1"

func Identity() protocol.SurfaceBuildIdentity {
	return protocol.SurfaceBuildIdentity{ComponentVersion: Version, SourceRevision: SourceRevision, BuildId: BuildID, Target: Target}
}

func Valid() bool {
	return SourceState == "verified_checkout" && ToolkitPatch == RequiredToolkitPatch && control.SameBuild(Identity(), Identity())
}

func Report() any {
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
	}{"surface", Version, SourceRevision, SourceState, BuildID, protocol.Version, MyGoVersion, MyGoSource, Target, ToolkitPatch}
}
