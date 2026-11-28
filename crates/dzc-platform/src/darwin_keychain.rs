use dzc_core::crypto::{AesTextFormat, EncryptionError, KeyProvider};
use rand::{rngs::OsRng, RngCore};
use security_framework::os::macos::keychain::{KeychainUserInteractionLock, SecKeychain};
use std::sync::LazyLock;
use uuid::Uuid;
use zeroize::Zeroizing;

const SERVICE: &str = "org.dezirclip.history-key.v1";
const ITEM_NOT_FOUND: i32 = -25_300;
const DUPLICATE_ITEM: i32 = -25_299;

// The program holding a provider is a headless helper. It must never wait
// for Keychain UI while servicing a clipboard, storage or IPC request.
static NO_INTERACTION: LazyLock<Result<KeychainUserInteractionLock, EncryptionError>> =
    LazyLock::new(|| {
        SecKeychain::disable_user_interaction().map_err(|_| EncryptionError::KeyUnavailable)
    });

/// An explicitly selected file-based keychain in the launcher's user context.
/// The stable profile ID comes from the Host's profile metadata.
pub struct DarwinKeyProvider {
    keychain: SecKeychain,
    account: String,
}

impl DarwinKeyProvider {
    pub fn for_profile(keychain: SecKeychain, profile_id: Uuid) -> Result<Self, EncryptionError> {
        if profile_id.is_nil() {
            return Err(EncryptionError::KeyUnavailable);
        }
        NO_INTERACTION.as_ref().map_err(|error| *error)?;
        Ok(Self {
            keychain,
            account: profile_id.to_string(),
        })
    }

    fn read_key(&self) -> Result<Option<Zeroizing<[u8; 32]>>, EncryptionError> {
        match self.keychain.find_generic_password(SERVICE, &self.account) {
            Ok((secret, _)) => {
                let mut key = Zeroizing::new([0u8; 32]);
                let password = secret.as_ref();
                if password.len() != key.len() {
                    return Err(EncryptionError::KeyUnavailable);
                }
                key.copy_from_slice(password);
                Ok(Some(key))
            }
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
            Err(_) => Err(EncryptionError::KeyUnavailable),
        }
    }

    pub fn verify_existing(&self) -> Result<(), EncryptionError> {
        self.read_key()?.map(|_| ()).ok_or(EncryptionError::KeyUnavailable)
    }

    /// Invoke only during previously uninitialized profile creation. An existing
    /// profile with a missing, locked or malformed key must surface the failure.
    pub fn provision_new_profile(&self) -> Result<(), EncryptionError> {
        if self.read_key()?.is_some() {
            return Ok(());
        }
        let mut key = Zeroizing::new([0u8; 32]);
        OsRng
            .try_fill_bytes(key.as_mut())
            .map_err(|_| EncryptionError::KeyUnavailable)?;
        match self
            .keychain
            .add_generic_password(SERVICE, &self.account, key.as_ref())
        {
            Ok(_) => Ok(()),
            // Another initializer for the same new profile may win the add.
            // Never update a password found in the keychain.
            Err(error) if error.code() == DUPLICATE_ITEM => self
                .read_key()?
                .map(|_| ())
                .ok_or(EncryptionError::KeyUnavailable),
            Err(_) => Err(EncryptionError::KeyUnavailable),
        }
    }
}

impl KeyProvider for DarwinKeyProvider {
    fn seal_text(&self, plain: &str) -> Result<String, EncryptionError> {
        let key = self.read_key()?.ok_or(EncryptionError::KeyUnavailable)?;
        let mut nonce = [0u8; 12];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| EncryptionError::EncryptionFailed)?;
        AesTextFormat::KeychainV1.seal(plain, &key, nonce)
    }

    fn open_text(&self, cipher: &str) -> Option<String> {
        AesTextFormat::KeychainV1.open(cipher, || self.read_key().ok().flatten())
    }
}
