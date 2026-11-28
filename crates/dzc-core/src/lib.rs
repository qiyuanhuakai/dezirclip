//! Shared business APIs. Native operations are supplied through small runtime ports.
pub mod api;
pub mod domain;
pub mod ports;
pub mod media_preview;
pub mod privacy;
pub use dzc_protocol as protocol;
pub use services::{capture, classification, qr, transforms};
/// The shared backup file format, usable without opening a history profile.
pub mod backup_format {
    pub use crate::services::backup::{
        decrypt_to_json, entries_from_json, export_to_encrypted, export_to_json,
        import_from_encrypted, import_from_json, prepare_export_entry, BackupError,
        ExportEntry, ExportFile, ImportMode, ImportSummary, EXPORT_VERSION,
    };
}
mod database;
mod infrastructure;
mod services;
/// Key protection adapters have no access to history connections or caches.
pub mod crypto {
    pub use crate::infrastructure::encryption::{
        decode_for_mutation, decode_stored_text, encrypt_sensitive, encrypt_value, encrypt_with_key,
        has_ciphertext_envelope,
        install_key_provider, AesTextFormat, EncryptedPayload, EncryptionError, KeyProvider, StoredText,
        ENCRYPT_PREFIX, KEYCHAIN_ENCRYPT_PREFIX, LINUX_ENCRYPT_PREFIX,
    };
}
/// Local file and attachment operations shared by adapters.
pub mod filesystem {
    pub use crate::infrastructure::attachments::{image_bytes, path_from_uri};
    pub use crate::infrastructure::local_file::*;
}
