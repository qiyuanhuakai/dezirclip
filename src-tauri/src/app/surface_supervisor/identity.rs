use dzc_host::wire::{SurfaceBuildIdentity, VERSION};
use serde::Deserialize;
pub(super) const TOOLKIT_PATCH: &str = "mygo-v0.6.4-no-startup-cleanup-v1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    component: String,
    component_version: String,
    source_revision: String,
    source_state: String,
    build_id: String,
    target: String,
    protocol_version: dzc_host::wire::ProtocolVersion,
    mygo_version: String,
    mygo_source_revision: String,
    toolkit_patch_id: String,
}
pub(super) fn validate(raw: &[u8], expected: &SurfaceBuildIdentity) -> Result<(), String> {
    let value: Identity = serde_json::from_slice(raw).map_err(|_| "SurfaceBuildMismatch")?;
    let build = SurfaceBuildIdentity {
        component_version: value.component_version,
        source_revision: value.source_revision,
        build_id: value.build_id,
        target: value.target,
    };
    if value.component != "surface"
        || value.source_state != "verified_checkout"
        || value.protocol_version != VERSION
        || value.mygo_version != "v0.6.4"
        || value.mygo_source_revision != "6255ea40139d06e0762a7c4a32626148ba410478"
        || value.toolkit_patch_id != TOOLKIT_PATCH
        || !dzc_core::protocol::surface_control::same_build(&build, expected)
    {
        return Err("SurfaceBuildMismatch".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_expected_clean_build_and_exact_cleanup_disabled_toolkit_can_start() {
        let build = SurfaceBuildIdentity {
            component_version: "0.2.0-alpha.5".into(),
            source_revision: "a".repeat(40),
            build_id: format!("0.2.0-alpha.5@{}", "a".repeat(40)),
            target: "x86_64-pc-windows-msvc".into(),
        };
        let valid = serde_json::json!({"component":"surface","component_version":build.component_version,"source_revision":build.source_revision,"build_id":build.build_id,"target":build.target,
            "source_state":"verified_checkout","protocol_version":{"major":1,"minor":0},"mygo_version":"v0.6.4","mygo_source_revision":"6255ea40139d06e0762a7c4a32626148ba410478","toolkit_patch_id":TOOLKIT_PATCH});
        validate(&serde_json::to_vec(&valid).unwrap(), &build).unwrap();
        for key in [
            "component",
            "component_version",
            "source_revision",
            "source_state",
            "build_id",
            "target",
            "mygo_version",
            "mygo_source_revision",
            "toolkit_patch_id",
        ] {
            let mut bad = valid.clone();
            bad[key] = "wrong".into();
            assert!(
                validate(&serde_json::to_vec(&bad).unwrap(), &build).is_err(),
                "{key}"
            );
        }
        let mut old = valid.clone();
        old.as_object_mut().unwrap().remove("toolkit_patch_id");
        assert!(validate(&serde_json::to_vec(&old).unwrap(), &build).is_err());
        let mut wrong = valid;
        wrong["protocol_version"]["major"] = 2.into();
        assert!(validate(&serde_json::to_vec(&wrong).unwrap(), &build).is_err());
    }
}
