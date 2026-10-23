use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreErrorCode {
    Validation,
    Storage,
    KeyUnavailable,
    EncryptionFailed,
    SensitivePersistenceRejected,
    ProtectedRecordUnavailable,
    ClipboardRecordUnavailable,
    PlatformUnavailable,
    PrivacyStateUnavailable,
    PrivacyEpochChanged,
    PrivacySessionActive,
    ManualPersistenceConfirmationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError {
    pub code: CoreErrorCode,
    message: String,
}

impl CoreError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            code: CoreErrorCode::Validation,
            message: message.into(),
        }
    }

    /// A public value never includes storage paths, SQL or user-supplied diagnostics.
    pub fn to_protocol(&self) -> dzc_protocol::history::DzcError {
        use dzc_protocol::history::DzcErrorCode as Wire;
        let code = match self.code {
            CoreErrorCode::Validation => Wire::InvalidRequest,
            CoreErrorCode::Storage => Wire::StorageFailed,
            CoreErrorCode::KeyUnavailable => Wire::KeyUnavailable,
            CoreErrorCode::EncryptionFailed => Wire::EncryptionFailed,
            CoreErrorCode::SensitivePersistenceRejected => Wire::SensitivePersistenceRejected,
            CoreErrorCode::ProtectedRecordUnavailable => Wire::ProtectedRecordUnavailable,
            CoreErrorCode::ClipboardRecordUnavailable => Wire::EntryUnavailable,
            CoreErrorCode::PlatformUnavailable => Wire::PlatformUnavailable,
            CoreErrorCode::PrivacyStateUnavailable => Wire::StorageFailed,
            CoreErrorCode::PrivacyEpochChanged | CoreErrorCode::PrivacySessionActive | CoreErrorCode::ManualPersistenceConfirmationRequired => Wire::InvalidRequest,
        };
        dzc_protocol::history::DzcError { code }
    }
}
impl From<String> for CoreError {
    fn from(message: String) -> Self {
        use CoreErrorCode::*;
        let code = match message.as_str() {
            "KeyUnavailable" => KeyUnavailable,
            "EncryptionFailed" => EncryptionFailed,
            "SensitivePersistenceRejected" => SensitivePersistenceRejected,
            "ProtectedRecordUnavailable" => ProtectedRecordUnavailable,
            "ClipboardRecordUnavailable" => ClipboardRecordUnavailable,
            "PrivacyStateUnavailable" => PrivacyStateUnavailable,
            "PrivacyEpochChanged" => PrivacyEpochChanged,
            "PrivacySessionActive" => PrivacySessionActive,
            "ManualPersistenceConfirmationRequired" => ManualPersistenceConfirmationRequired,
            _ => Storage,
        };
        Self { code, message }
    }
}
impl From<dzc_protocol::PlatformError> for CoreError {
    fn from(error: dzc_protocol::PlatformError) -> Self {
        Self {
            code: CoreErrorCode::PlatformUnavailable,
            message: error.to_string(),
        }
    }
}
impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for CoreError {}
impl From<CoreError> for String {
    fn from(error: CoreError) -> Self {
        error.to_string()
    }
}
pub type CoreResult<T> = Result<T, CoreError>;
