use base64::Engine;

#[cfg(windows)]
use std::ffi::c_void;

#[cfg(windows)]
type BOOL = i32;
#[cfg(windows)]
type DWORD = u32;

#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct DATA_BLOB {
    cbData: DWORD,
    pbData: *mut u8,
}

#[cfg(windows)]
const CRYPTPROTECT_UI_FORBIDDEN: DWORD = 0x1;

#[cfg(windows)]
#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(
        p_data_in: *mut DATA_BLOB,
        sz_data_descr: *const u16,
        p_optional_entropy: *mut DATA_BLOB,
        pv_reserved: *mut c_void,
        p_prompt_struct: *mut c_void,
        dw_flags: DWORD,
        p_data_out: *mut DATA_BLOB,
    ) -> BOOL;

    fn CryptUnprotectData(
        p_data_in: *mut DATA_BLOB,
        ppsz_data_descr: *mut *mut u16,
        p_optional_entropy: *mut DATA_BLOB,
        pv_reserved: *mut c_void,
        p_prompt_struct: *mut c_void,
        dw_flags: DWORD,
        p_data_out: *mut DATA_BLOB,
    ) -> BOOL;
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(hmem: *mut c_void) -> *mut c_void;
}

pub const ENCRYPT_PREFIX: &str = "dpapi:";
pub const LINUX_ENCRYPT_PREFIX: &str = "linux:";

pub fn is_encrypted_value(value: &str) -> bool {
    value.starts_with(ENCRYPT_PREFIX) || value.starts_with(LINUX_ENCRYPT_PREFIX)
}

#[cfg(windows)]
pub fn encrypt_value(plain: &str) -> Option<String> {
    let bytes = plain.as_bytes();
    let mut in_blob = DATA_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut out_blob = DATA_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &mut in_blob,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };
    if ok != 0 {
        let out = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
        let encoded = base64::engine::general_purpose::STANDARD.encode(out);
        unsafe {
            let _ = LocalFree(out_blob.pbData as _);
        }
        Some(format!("{}{}", ENCRYPT_PREFIX, encoded))
    } else {
        None
    }
}

#[cfg(windows)]
pub fn decrypt_value(cipher: &str) -> Option<String> {
    let payload = cipher.strip_prefix(ENCRYPT_PREFIX)?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;
    let mut in_blob = DATA_BLOB {
        cbData: decoded.len() as u32,
        pbData: decoded.as_ptr() as *mut u8,
    };
    let mut out_blob = DATA_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &mut in_blob,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };
    if ok != 0 {
        let out = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
        let result = String::from_utf8(out.to_vec()).ok();
        unsafe {
            let _ = LocalFree(out_blob.pbData as _);
        }
        result
    } else {
        None
    }
}

#[cfg(not(windows))]
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};

const MASTER_KEY_SERVICE: &str = "dezirclip";
const MASTER_KEY_ACCOUNT: &str = "encryption-master-key";

// How a read of the stored master key came back, kept apart from the keyring
// error type so the decision built on it can be reasoned about -- and tested --
// on any platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MasterKeyRead {
    /// A key is stored and decoded to 32 bytes.
    Present,
    /// Nothing is stored under the service and account names. This is the only
    /// outcome that means "there is no key yet".
    Missing,
    /// The read failed for some other reason: a locked keyring, no session bus,
    /// a service that refused the request, storage the process cannot read.
    Unavailable,
}

// What to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MasterKeyPlan {
    Use,
    Create,
    Unavailable,
}

/// The decision table, and the bug it exists to keep from coming back.
///
/// Treating every failed read as "no key yet" makes the next write generate a
/// fresh key over the old one, and everything encrypted under the old key stops
/// being readable -- permanently, with no way back. Only a store that reports
/// nothing is allowed to be written to.
fn plan_master_key(read: MasterKeyRead) -> MasterKeyPlan {
    match read {
        MasterKeyRead::Present => MasterKeyPlan::Use,
        MasterKeyRead::Missing => MasterKeyPlan::Create,
        // The read says nothing about whether a key is stored. Writing here is
        // how the previous version destroyed data.
        MasterKeyRead::Unavailable => MasterKeyPlan::Unavailable,
    }
}

/// Decode a stored key. A value that is not exactly 32 base64-encoded bytes is
/// not a key, and is treated as an unreadable store rather than as absence.
fn decode_master_key(stored: &str) -> Option<[u8; 32]> {
    let decoded = base64::engine::general_purpose::STANDARD.decode(stored).ok()?;
    if decoded.len() != 32 {
        return None;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Some(key)
}

#[cfg(not(windows))]
fn classify_read_error(err: &keyring::Error) -> MasterKeyRead {
    match err {
        keyring::Error::NoEntry => MasterKeyRead::Missing,
        _ => MasterKeyRead::Unavailable,
    }
}

#[cfg(not(windows))]
fn read_master_key() -> Result<[u8; 32], MasterKeyRead> {
    let entry =
        keyring::Entry::new(MASTER_KEY_SERVICE, MASTER_KEY_ACCOUNT).map_err(|err| {
            crate::warn!("[encryption] master key store unavailable: {}", err);
            classify_read_error(&err)
        })?;
    match entry.get_password() {
        Ok(stored) => decode_master_key(&stored).ok_or_else(|| {
            crate::warn!("[encryption] stored master key is not 32 base64 bytes");
            MasterKeyRead::Unavailable
        }),
        Err(err) => {
            let read = classify_read_error(&err);
            if read == MasterKeyRead::Unavailable {
                crate::warn!("[encryption] reading the master key failed: {}", err);
            }
            Err(read)
        }
    }
}

/// Cached only after a key has actually been decoded, so a keyring that was
/// briefly unavailable is asked again on the next call instead of being
/// remembered as "there is no key".
#[cfg(not(windows))]
static MASTER_KEY: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();

/// Two threads that both find nothing stored would otherwise each generate a
/// key, and whichever wrote last would strand everything encrypted under the
/// other's. Creation is not a hot path, so the plain lock costs nothing that
/// matters.
#[cfg(not(windows))]
static MASTER_KEY_CREATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(not(windows))]
fn create_master_key() -> Option<[u8; 32]> {
    let _guard = MASTER_KEY_CREATE_LOCK.lock().ok()?;
    // Another thread may have created the key between our read and this lock.
    if let Some(key) = MASTER_KEY.get() {
        return Some(*key);
    }
    let entry = keyring::Entry::new(MASTER_KEY_SERVICE, MASTER_KEY_ACCOUNT).ok()?;
    let mut key = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&key);
    // A store that refuses the write means there is nowhere durable to keep the
    // key. Handing one back anyway would encrypt with a key the next login
    // cannot read, which loses the data instead of leaving it unencrypted.
    if let Err(err) = entry.set_password(&encoded) {
        crate::warn!("[encryption] could not store the master key: {}", err);
        return None;
    }
    Some(key)
}

/// The key used for writing. Returns `None` when the store cannot be read and
/// must not be written, which the callers turn into "leave the value as it is".
#[cfg(not(windows))]
fn master_key() -> Option<[u8; 32]> {
    if let Some(key) = MASTER_KEY.get() {
        return Some(*key);
    }
    let (read, decoded) = match read_master_key() {
        Ok(key) => (MasterKeyRead::Present, Some(key)),
        Err(reason) => (reason, None),
    };
    let key = match plan_master_key(read) {
        MasterKeyPlan::Use => decoded?,
        MasterKeyPlan::Create => create_master_key()?,
        MasterKeyPlan::Unavailable => return None,
    };
    let _ = MASTER_KEY.set(key);
    Some(key)
}

/// The key used for reading. It never creates one: decryption has to be able to
/// fail honestly rather than replace the key it is about to be measured
/// against.
#[cfg(not(windows))]
fn existing_master_key() -> Option<[u8; 32]> {
    if let Some(key) = MASTER_KEY.get() {
        return Some(*key);
    }
    let key = read_master_key().ok()?;
    let _ = MASTER_KEY.set(key);
    Some(key)
}

#[cfg(not(windows))]
pub fn encrypt_value(plain: &str) -> Option<String> {
    let key = master_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;

    let mut nonce_bytes = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, plain.as_bytes()).ok()?;

    let mut result = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    let encoded = base64::engine::general_purpose::STANDARD.encode(&result);
    Some(format!("{}{}", LINUX_ENCRYPT_PREFIX, encoded))
}

#[cfg(not(windows))]
pub fn decrypt_value(cipher: &str) -> Option<String> {
    let payload = cipher.strip_prefix(LINUX_ENCRYPT_PREFIX)?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;

    if decoded.len() < 12 {
        return None;
    }

    let key = existing_master_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;

    let nonce = Nonce::from_slice(&decoded[..12]);
    let plaintext = cipher.decrypt(nonce, &decoded[12..]).ok()?;

    String::from_utf8(plaintext).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded_key(byte: u8) -> String {
        base64::engine::general_purpose::STANDARD.encode([byte; 32])
    }

    // The regression this table prevents: a read that failed for a reason
    // other than "nothing stored" must never be answered by writing a new key
    // over whatever is there.
    #[test]
    fn only_an_absent_entry_authorises_creating_a_key() {
        assert_eq!(plan_master_key(MasterKeyRead::Present), MasterKeyPlan::Use);
        assert_eq!(plan_master_key(MasterKeyRead::Missing), MasterKeyPlan::Create);
        assert_eq!(
            plan_master_key(MasterKeyRead::Unavailable),
            MasterKeyPlan::Unavailable
        );
        assert_ne!(
            plan_master_key(MasterKeyRead::Unavailable),
            MasterKeyPlan::Create,
            "an unavailable store must never be treated as an empty one"
        );
    }

    #[test]
    fn a_stored_key_round_trips_through_decode() {
        let key = decode_master_key(&encoded_key(7)).expect("32 bytes must decode");
        assert_eq!(key, [7u8; 32]);
    }

    #[test]
    fn a_stored_value_that_is_not_a_key_is_rejected() {
        // Wrong length, valid base64.
        let short = base64::engine::general_purpose::STANDARD.encode([1u8; 16]);
        assert!(decode_master_key(&short).is_none());
        // Right length, not base64.
        assert!(decode_master_key("not base64 at all, but long enough!!!!")
            .is_none());
        // Empty.
        assert!(decode_master_key("").is_none());
    }

    #[test]
    fn a_rejected_stored_value_is_never_read_as_absence() {
        // Unavailable, and Unavailable is what must not lead to a write.
        let broken = base64::engine::general_purpose::STANDARD.encode([3u8; 31]);
        assert!(decode_master_key(&broken).is_none());
        assert_eq!(
            plan_master_key(MasterKeyRead::Unavailable),
            MasterKeyPlan::Unavailable
        );
    }
}
