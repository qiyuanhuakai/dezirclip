use base64::Engine;

pub const ENCRYPT_PREFIX: &str = "dpapi:";
pub const LINUX_ENCRYPT_PREFIX: &str = "linux:";

pub fn is_encrypted_value(value: &str) -> bool {
    value.starts_with(ENCRYPT_PREFIX) || value.starts_with(LINUX_ENCRYPT_PREFIX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionError {
    KeyUnavailable,
    EncryptionFailed,
    SensitivePersistenceRejected,
    ProtectedRecordUnavailable,
}

impl std::fmt::Display for EncryptionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::KeyUnavailable => "KeyUnavailable",
            Self::EncryptionFailed => "EncryptionFailed",
            Self::SensitivePersistenceRejected => "SensitivePersistenceRejected",
            Self::ProtectedRecordUnavailable => "ProtectedRecordUnavailable",
        })
    }
}

impl std::error::Error for EncryptionError {}

#[derive(Debug)]
pub struct EncryptedPayload(String);

impl EncryptedPayload {
    pub fn into_string(self) -> String {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(value: String) -> Self {
        Self(value)
    }
}

/// The input is plaintext, even when it resembles a stored ciphertext prefix.
pub fn encrypt_sensitive(plain: &str) -> Result<EncryptedPayload, EncryptionError> {
    encrypt_sensitive_with(plain, cfg!(feature = "portable"), encrypt_platform)
}

fn encrypt_sensitive_with(
    plain: &str,
    portable: bool,
    encrypt: impl FnOnce(&str) -> Result<String, EncryptionError>,
) -> Result<EncryptedPayload, EncryptionError> {
    if portable {
        return Err(EncryptionError::SensitivePersistenceRejected);
    }
    encrypt(plain).map(EncryptedPayload)
}

/// Mutations must distinguish an unavailable protected record from ordinary text.
pub fn decode_for_mutation(value: &str) -> Result<String, EncryptionError> {
    match decode_stored_text(value) {
        StoredText::Plain(plain) => Ok(plain.to_owned()),
        StoredText::Decrypted(plain) => Ok(plain),
        StoredText::Failed => Err(EncryptionError::ProtectedRecordUnavailable),
    }
}

/// Rewrite stored fields without ever substituting rewritten plaintext for ciphertext.
pub fn rewrite_stored_text(
    value: &str,
    rewrite: impl FnOnce(&str) -> Option<String>,
) -> Result<Option<String>, EncryptionError> {
    rewrite_decoded_text(decode_stored_text(value), rewrite, encrypt_sensitive)
}

fn rewrite_decoded_text(
    decoded: StoredText<'_>,
    rewrite: impl FnOnce(&str) -> Option<String>,
    encrypt: impl FnOnce(&str) -> Result<EncryptedPayload, EncryptionError>,
) -> Result<Option<String>, EncryptionError> {
    match decoded {
        StoredText::Plain(value) => Ok(rewrite(value)),
        StoredText::Decrypted(value) => rewrite(&value)
            .map(|updated| encrypt(&updated).map(EncryptedPayload::into_string))
            .transpose(),
        StoredText::Failed => Err(EncryptionError::ProtectedRecordUnavailable),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoredText<'a> {
    Plain(&'a str),
    Decrypted(String),
    Failed,
}

/// Keep the result of decoding a stored field separate from its plaintext.
pub fn decode_stored_text(value: &str) -> StoredText<'_> {
    decode_stored_text_with(value, decrypt_value)
}

fn decode_stored_text_with(
    value: &str,
    decrypt: impl FnOnce(&str) -> Option<String>,
) -> StoredText<'_> {
    if !is_encrypted_value(value) {
        return StoredText::Plain(value);
    }
    if let Some(plain) = decrypt(value) {
        return StoredText::Decrypted(plain);
    }
    if has_ciphertext_envelope(value) {
        StoredText::Failed
    } else {
        StoredText::Plain(value)
    }
}

fn has_ciphertext_envelope(value: &str) -> bool {
    let (linux, payload) = if let Some(payload) = value.strip_prefix(LINUX_ENCRYPT_PREFIX) {
        (true, payload)
    } else if let Some(payload) = value.strip_prefix(ENCRYPT_PREFIX) {
        (false, payload)
    } else {
        return false;
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload) else {
        return false;
    };
    if linux {
        // AES-GCM stores a 12-byte nonce followed by ciphertext and a 16-byte tag.
        bytes.len() >= 12 + 16
    } else {
        valid_dpapi_envelope(&bytes).is_some()
    }
}

fn valid_dpapi_envelope(data: &[u8]) -> Option<()> {
    struct Reader<'a> {
        data: &'a [u8],
        position: usize,
    }
    impl<'a> Reader<'a> {
        fn take(&mut self, count: usize) -> Option<&'a [u8]> {
            let end = self.position.checked_add(count)?;
            let bytes = self.data.get(self.position..end)?;
            self.position = end;
            Some(bytes)
        }
        fn word(&mut self) -> Option<u32> {
            Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
        }
        fn blob(&mut self) -> Option<&'a [u8]> {
            let count = usize::try_from(self.word()?).ok()?;
            self.take(count)
        }
    }
    // DPAPI_BLOB layout: https://github.com/fortra/impacket/blob/master/impacket/dpapi.py
    const PROVIDER: [u8; 16] = [
        0xd0, 0x8c, 0x9d, 0xdf, 0x01, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0x00, 0xc0, 0x4f, 0xc2, 0x97,
        0xeb,
    ];
    let mut reader = Reader { data, position: 0 };
    if reader.word()? != 1 || reader.take(16)? != PROVIDER || reader.word()? != 1 {
        return None;
    }
    reader.take(16)?; // Master key GUID.
    reader.word()?; // Flags.
    if reader.blob()?.len() % 2 != 0 {
        return None;
    }
    let cipher_algorithm = reader.word()?;
    let cipher_bits = reader.word()?;
    if cipher_algorithm & 0xe000 != 0x6000
        || cipher_bits == 0
        || cipher_bits > 4096
        || cipher_bits % 8 != 0
    {
        return None;
    }
    if reader.blob()?.is_empty() {
        return None;
    } // Salt.
    reader.blob()?; // Optional HMAC key.
    let hash_algorithm = reader.word()?;
    let hash_bits = reader.word()?;
    if hash_algorithm & 0xe000 != 0x8000 || hash_bits == 0 || hash_bits > 4096 || hash_bits % 8 != 0
    {
        return None;
    }
    reader.blob()?; // HMAC data.
    if reader.blob()?.is_empty() || reader.blob()?.is_empty() || reader.position != data.len() {
        return None;
    }
    Some(())
}

#[cfg(test)]
mod stored_text_tests {
    use super::*;

    fn dpapi_blob() -> Vec<u8> {
        fn word(bytes: &mut Vec<u8>, value: u32) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fn blob(bytes: &mut Vec<u8>, value: &[u8]) {
            word(bytes, value.len() as u32);
            bytes.extend_from_slice(value);
        }
        let mut bytes = Vec::new();
        word(&mut bytes, 1);
        bytes.extend_from_slice(&[
            0xd0, 0x8c, 0x9d, 0xdf, 0x01, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0x00, 0xc0, 0x4f, 0xc2,
            0x97, 0xeb,
        ]);
        word(&mut bytes, 1);
        bytes.extend_from_slice(&[42; 16]);
        word(&mut bytes, 0);
        blob(&mut bytes, &[0, 0]);
        word(&mut bytes, 0x6610);
        word(&mut bytes, 256);
        blob(&mut bytes, &[1; 32]);
        blob(&mut bytes, &[]);
        word(&mut bytes, 0x800e);
        word(&mut bytes, 512);
        blob(&mut bytes, &[2; 32]);
        blob(&mut bytes, &[3; 16]);
        blob(&mut bytes, &[4; 64]);
        bytes
    }

    #[test]
    fn ordinary_prefix_text_and_short_or_non_dpapi_blobs_are_plain() {
        for value in [
            "linux: kernel notes",
            "dpapi: configuration",
            "linux:<p>HTML</p>",
            "dpapi:",
            "linux:unreadable",
        ] {
            assert_eq!(
                decode_stored_text_with(value, |_| None),
                StoredText::Plain(value)
            );
        }
        for length in 0..28 {
            let value = format!(
                "linux:{}",
                base64::engine::general_purpose::STANDARD.encode(vec![42; length])
            );
            assert_eq!(
                decode_stored_text_with(&value, |_| None),
                StoredText::Plain(value.as_str())
            );
        }
        let value = format!(
            "dpapi:{}",
            base64::engine::general_purpose::STANDARD.encode([42; 200])
        );
        assert_eq!(
            decode_stored_text_with(&value, |_| None),
            StoredText::Plain(value.as_str())
        );
    }

    #[test]
    fn failed_complete_envelopes_remain_distinct_from_plaintext() {
        for value in [
            format!(
                "linux:{}",
                base64::engine::general_purpose::STANDARD.encode([42; 28])
            ),
            format!(
                "dpapi:{}",
                base64::engine::general_purpose::STANDARD.encode(dpapi_blob())
            ),
        ] {
            assert_eq!(
                decode_stored_text_with(&value, |_| None),
                StoredText::Failed
            );
            for plain in ["linux: kernel notes", "dpapi:<p>HTML</p>"] {
                assert_eq!(
                    decode_stored_text_with(&value, |_| Some(plain.to_string())),
                    StoredText::Decrypted(plain.to_string())
                );
            }
        }
    }

    #[test]
    fn dpapi_requires_provider_guid_and_all_length_prefixed_fields() {
        let bytes = dpapi_blob();
        assert!(valid_dpapi_envelope(&bytes).is_some());
        for length in 0..bytes.len() {
            assert!(
                valid_dpapi_envelope(&bytes[..length]).is_none(),
                "truncated at {length}"
            );
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(valid_dpapi_envelope(&trailing).is_none());
        let mut wrong_provider = bytes.clone();
        wrong_provider[4] ^= 1;
        assert!(valid_dpapi_envelope(&wrong_provider).is_none());
        let mut oversized = bytes;
        oversized[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(valid_dpapi_envelope(&oversized).is_none());
    }
}

pub fn encrypt_value(plain: &str) -> Option<String> {
    encrypt_platform(plain).ok()
}

pub fn encrypt_with_key(
    plain: &str,
    key: impl FnOnce() -> Result<[u8; 32], EncryptionError>,
    fill_nonce: impl FnOnce(&mut [u8]) -> Result<(), EncryptionError>,
) -> Result<String, EncryptionError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    let key = key()?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| EncryptionError::EncryptionFailed)?;
    let mut nonce_bytes = [0u8; 12];
    fill_nonce(&mut nonce_bytes)?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plain.as_bytes())
        .map_err(|_| EncryptionError::EncryptionFailed)?;
    let mut sealed = nonce_bytes.to_vec();
    sealed.extend_from_slice(&ciphertext);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&sealed);
    Ok(format!("{}{}", LINUX_ENCRYPT_PREFIX, encoded))
}

#[cfg(test)]
mod sensitive_failure_tests {
    use super::*;

    #[test]
    fn encryption_failures_have_stable_content_free_codes() {
        for error in [
            EncryptionError::KeyUnavailable,
            EncryptionError::EncryptionFailed,
            EncryptionError::SensitivePersistenceRejected,
            EncryptionError::ProtectedRecordUnavailable,
        ] {
            let result = encrypt_sensitive_with("private probe", false, |_| Err(error));
            assert_eq!(result.unwrap_err(), error);
            assert!(!error.to_string().contains("private probe"));
        }
        let result = encrypt_sensitive_with("private probe", true, |_| {
            panic!("portable must reject before encryption")
        });
        assert_eq!(
            result.unwrap_err(),
            EncryptionError::SensitivePersistenceRejected
        );
    }

    #[test]
    fn protected_path_rewrite_never_returns_plaintext_on_failure() {
        for error in [
            EncryptionError::KeyUnavailable,
            EncryptionError::EncryptionFailed,
        ] {
            let result = rewrite_decoded_text(
                StoredText::Decrypted("old/private/path".to_string()),
                |plain| Some(plain.replace("old/", "new/")),
                |_| Err(error),
            );
            assert_eq!(result, Err(error));
        }
        assert_eq!(
            rewrite_decoded_text(
                StoredText::Failed,
                |_| panic!("unavailable ciphertext must not be rewritten"),
                |_| panic!("unavailable ciphertext must not be encrypted")
            ),
            Err(EncryptionError::ProtectedRecordUnavailable)
        );
        assert_eq!(
            rewrite_decoded_text(
                StoredText::Decrypted("unrelated".to_string()),
                |_| None,
                |_| panic!("unchanged ciphertext needs no write")
            ),
            Ok(None)
        );
        assert_eq!(
            rewrite_stored_text("linux:ordinary old/path", |plain| Some(
                plain.replace("old/", "new/")
            ))
            .unwrap(),
            Some("linux:ordinary new/path".to_string())
        );
    }

    #[test]
    fn prefixes_never_exempt_new_plaintext_from_encryption() {
        for value in ["linux:ordinary text", "dpapi:ordinary text"] {
            let result = encrypt_sensitive_with(value, false, |plain| {
                assert_eq!(plain, value);
                Ok("sealed value".to_string())
            })
            .unwrap();
            assert_eq!(result.into_string(), "sealed value");
        }
    }

    #[test]
    fn key_and_nonce_failures_propagate_without_producing_ciphertext() {
        let result = encrypt_with_key(
            "private probe",
            || Err(EncryptionError::KeyUnavailable),
            |_| panic!("no key"),
        );
        assert_eq!(result.unwrap_err(), EncryptionError::KeyUnavailable);
        let result = encrypt_with_key(
            "private probe",
            || Ok([7; 32]),
            |_| Err(EncryptionError::EncryptionFailed),
        );
        assert_eq!(result.unwrap_err(), EncryptionError::EncryptionFailed);
    }

    #[test]
    fn unreadable_protected_text_cannot_be_mutated_as_plaintext() {
        let sealed = format!(
            "linux:{}",
            base64::engine::general_purpose::STANDARD.encode([0u8; 28])
        );
        assert_eq!(
            decode_for_mutation(&sealed),
            Err(EncryptionError::ProtectedRecordUnavailable)
        );
        assert_eq!(
            decode_for_mutation("dpapi:ordinary text"),
            Ok("dpapi:ordinary text".to_string())
        );
        assert_eq!(
            decode_for_mutation("linux:ordinary text"),
            Ok("linux:ordinary text".to_string())
        );
    }
}

/// OS-owned keys may be opaque (DPAPI); providers seal and open complete text values.
/// Sealed values must use a complete supported ciphertext envelope.
pub trait KeyProvider: Send + Sync {
    fn seal_text(&self, plain: &str) -> Result<String, EncryptionError>;
    fn open_text(&self, cipher: &str) -> Option<String>;
}

static KEY_PROVIDER: std::sync::OnceLock<&'static dyn KeyProvider> = std::sync::OnceLock::new();

pub fn install_key_provider(provider: &'static dyn KeyProvider) -> Result<(), &'static str> {
    KEY_PROVIDER
        .set(provider)
        .map_err(|_| "KeyProviderAlreadyConfigured")
}

fn encrypt_platform(plain: &str) -> Result<String, EncryptionError> {
    seal_with_provider(plain, provider()?)
}

fn seal_with_provider(plain: &str, provider: &dyn KeyProvider) -> Result<String, EncryptionError> {
    let sealed = provider.seal_text(plain)?;
    if !has_ciphertext_envelope(&sealed) {
        return Err(EncryptionError::EncryptionFailed);
    }
    Ok(sealed)
}

pub fn decrypt_value(cipher: &str) -> Option<String> {
    provider().ok()?.open_text(cipher)
}

fn provider() -> Result<&'static dyn KeyProvider, EncryptionError> {
    if let Some(provider) = KEY_PROVIDER.get() {
        return Ok(*provider);
    }
    #[cfg(test)]
    {
        return Ok(&TEST_PROVIDER);
    }
    #[cfg(not(test))]
    {
        Err(EncryptionError::KeyUnavailable)
    }
}

#[cfg(test)]
struct TestKeyProvider;
#[cfg(test)]
static TEST_PROVIDER: TestKeyProvider = TestKeyProvider;
#[cfg(test)]
impl KeyProvider for TestKeyProvider {
    fn seal_text(&self, plain: &str) -> Result<String, EncryptionError> {
        encrypt_with_key(
            plain,
            || Ok([37; 32]),
            |nonce| {
                nonce.fill(19);
                Ok(())
            },
        )
    }
    fn open_text(&self, cipher: &str) -> Option<String> {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce,
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(cipher.strip_prefix(LINUX_ENCRYPT_PREFIX)?)
            .ok()?;
        if bytes.len() < 12 {
            return None;
        }
        let key = Aes256Gcm::new_from_slice(&[37; 32]).ok()?;
        String::from_utf8(
            key.decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
                .ok()?,
        )
        .ok()
    }
}

#[cfg(test)]
mod key_provider_tests {
    use super::*;

    struct InvalidProvider(&'static str);

    impl KeyProvider for InvalidProvider {
        fn seal_text(&self, _: &str) -> Result<String, EncryptionError> {
            Ok(self.0.to_owned())
        }

        fn open_text(&self, _: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn provider_cannot_turn_sensitive_input_into_unprotected_storage() {
        for value in [
            "private probe",
            "dpapi:private probe",
            "linux:private probe",
        ] {
            assert_eq!(
                seal_with_provider("private probe", &InvalidProvider(value)),
                Err(EncryptionError::EncryptionFailed)
            );
        }
        let sealed = seal_with_provider("private probe", &TEST_PROVIDER).unwrap();
        assert_eq!(
            TEST_PROVIDER.open_text(&sealed).as_deref(),
            Some("private probe")
        );
    }
}
