//! Build identity values, not an IPC handshake or installed-component inventory.
use serde::{Deserialize, Serialize};
use crate::{ProtocolVersion, PROTOCOL_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Component { Desktop, Cli }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentVersion {
    pub component: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub metadata_version: u16,
    pub component: Component,
    pub component_version: String,
    pub build_id: Option<String>,
    pub source_revision: Option<String>,
    pub source_state: String,
    pub revision_origin: String,
    pub target: String,
    pub protocol_version: ProtocolVersion,
    pub database_schema_version: i32,
    /// Actually linked Rust components; this does not claim other executables exist.
    pub linked_components: Vec<ComponentVersion>,
}

impl BuildInfo {
    pub fn current(component: Component, database_schema_version: i32) -> Self {
        let revision = env!("DZC_SOURCE_REVISION");
        let version = env!("CARGO_PKG_VERSION");
        let revision = (revision != "unknown").then(|| revision.to_owned());
        Self {
            metadata_version: 1, component, component_version: version.to_owned(),
            build_id: revision.as_ref().map(|revision| format!("{version}@{revision}")),
            source_revision: revision, source_state: env!("DZC_SOURCE_STATE").into(),
            revision_origin: env!("DZC_REVISION_ORIGIN").into(), target: env!("DZC_BUILD_TARGET").into(),
            protocol_version: PROTOCOL_VERSION, database_schema_version,
            linked_components: ["dzc-core", "dzc-platform", "dzc-protocol"].into_iter()
                .map(|name| ComponentVersion { component: name.into(), version: version.into() }).collect(),
        }
    }

    /// A matching identifier from modified/unknown source never proves a build set.
    /// Future IPC must also negotiate roles and required capabilities.
    /// Metadata consistency is not remote authentication or a signature proof.
    pub fn same_verified_build_set(&self, peer: &Self) -> bool {
        let verified = |info: &Self| info.source_state == "verified_checkout"
            && info.source_revision.as_ref().is_some_and(|revision| revision.len() == 40
                && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                && info.build_id.as_deref() == Some(format!("{}@{revision}", info.component_version).as_str()));
        verified(self) && verified(peer) && self.build_id == peer.build_id
            && self.component_version == peer.component_version
            && self.protocol_version == peer.protocol_version
            && self.database_schema_version == peer.database_schema_version
            && self.linked_components == peer.linked_components
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modified_unknown_or_different_build_sets_are_never_confirmed() {
        let mut desktop = BuildInfo::current(Component::Desktop, 19);
        desktop.source_state = "verified_checkout".into();
        desktop.source_revision = Some("0123456789012345678901234567890123456789".into());
        desktop.build_id = Some(format!("{}@{}", desktop.component_version, desktop.source_revision.as_ref().unwrap()));
        let mut cli = desktop.clone(); cli.component = Component::Cli;
        assert!(desktop.same_verified_build_set(&cli));
        for state in ["modified_checkout", "unknown"] {
            cli.source_state = state.into(); assert!(!desktop.same_verified_build_set(&cli));
        }
        cli = desktop.clone(); cli.build_id = None;
        assert!(!desktop.same_verified_build_set(&cli));
        cli = desktop.clone(); cli.source_revision = None;
        assert!(!desktop.same_verified_build_set(&cli));
        cli = desktop.clone(); cli.database_schema_version += 1;
        assert!(!desktop.same_verified_build_set(&cli));
        cli = desktop.clone(); cli.component_version = "0.2.1".into();
        assert!(!desktop.same_verified_build_set(&cli));
        cli = desktop.clone(); cli.linked_components[0].version = "0.2.1".into();
        assert!(!desktop.same_verified_build_set(&cli));
    }
    #[test]
    fn component_inventory_does_not_claim_service_or_surface_is_installed() {
        let info = BuildInfo::current(Component::Cli, 19);
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["component"], "cli");
        assert_eq!(info.linked_components.len(), 3);
        assert!(!info.linked_components.iter().any(|item| item.component.contains("surface") || item.component.contains("service")));
    }
}
