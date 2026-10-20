//! Versioned owned values for explicit Core conversions. Transport and the
//! language-independent Wire Schema belong to the later IPC phase.
use crate::{ProtocolVersion, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Contains,
    Fuzzy,
    Regex,
    Fts5,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryQuery {
    pub protocol: ProtocolVersion,
    pub limit: u16,
    pub offset: u32,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub pinned: bool,
}

impl Default for SummaryQuery {
    fn default() -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            limit: 50,
            offset: 0,
            kind: None,
            tag: None,
            pinned: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub mode: SearchMode,
    pub page: SummaryQuery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryVisibility {
    Ordinary,
}

/// No body, HTML, image, attachment path, process path, icon or revision token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrySummary {
    pub id: i64,
    pub kind: String,
    pub preview: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub timestamp: i64,
    pub source_app: String,
    pub privacy_visibility: SummaryVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub protocol: ProtocolVersion,
    pub entries: Vec<EntrySummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DzcErrorCode {
    InvalidRequest,
    IncompatibleVersion,
    StorageFailed,
    KeyUnavailable,
    EncryptionFailed,
    SensitivePersistenceRejected,
    ProtectedRecordUnavailable,
    EntryUnavailable,
    PlatformUnavailable,
}

impl DzcErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::IncompatibleVersion => "incompatible_version",
            Self::StorageFailed => "storage_failed",
            Self::KeyUnavailable => "key_unavailable",
            Self::EncryptionFailed => "encryption_failed",
            Self::SensitivePersistenceRejected => "sensitive_persistence_rejected",
            Self::ProtectedRecordUnavailable => "protected_record_unavailable",
            Self::EntryUnavailable => "entry_unavailable",
            Self::PlatformUnavailable => "platform_unavailable",
        }
    }
}

/// Internal diagnostics and user-supplied values never enter this error value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DzcError {
    pub code: DzcErrorCode,
}

impl std::fmt::Display for DzcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code.as_str())
    }
}
impl std::error::Error for DzcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_values_roundtrip_without_transport_or_private_payload_fields() {
        let request = SearchRequest {
            query: "ordinary".to_owned(),
            mode: SearchMode::Contains,
            page: SummaryQuery::default(),
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["page"]["protocol"], serde_json::json!({"major":1,"minor":0}));
        assert_eq!(value["mode"], "contains");
        assert_eq!(serde_json::from_value::<SearchRequest>(value).unwrap(), request);

        let result = SearchResult { protocol: PROTOCOL_VERSION, entries: vec![] };
        assert_eq!(serde_json::to_value(result).unwrap(),
            serde_json::json!({"protocol":{"major":1,"minor":0},"entries":[]}));
    }

    #[test]
    fn error_codes_are_strings_and_display_never_uses_rust_type_names() {
        for code in [DzcErrorCode::InvalidRequest, DzcErrorCode::IncompatibleVersion,
            DzcErrorCode::StorageFailed, DzcErrorCode::KeyUnavailable,
            DzcErrorCode::EncryptionFailed, DzcErrorCode::SensitivePersistenceRejected,
            DzcErrorCode::ProtectedRecordUnavailable, DzcErrorCode::EntryUnavailable,
            DzcErrorCode::PlatformUnavailable] {
            let error = DzcError { code };
            assert_eq!(serde_json::to_value(error).unwrap(), serde_json::json!({"code":code.as_str()}));
            assert_eq!(error.to_string(), code.as_str());
        }
    }
}
