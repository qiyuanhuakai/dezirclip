//! Shared business APIs. Native operations are supplied through small runtime ports.
pub mod api;
pub mod domain;
pub mod ports;
pub mod privacy;
pub use dzc_protocol as protocol;
pub use services::capture;
mod database;
mod infrastructure;
mod services;
/// Key protection adapters have no access to history connections or caches.
pub mod crypto {
    pub use crate::infrastructure::encryption::{
        decode_for_mutation, decode_stored_text, encrypt_sensitive, encrypt_value, encrypt_with_key,
        install_key_provider, EncryptedPayload, EncryptionError, KeyProvider, StoredText,
        ENCRYPT_PREFIX, LINUX_ENCRYPT_PREFIX,
    };
}
/// Bounded local file operations shared by platform adapters.
pub mod filesystem {
    pub use crate::infrastructure::local_file::*;
}
#[cfg(feature = "legacy-bridge")]
#[doc(hidden)]
pub mod legacy;
