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

// Why the key is looked up in two places, in enough detail that it does not
// have to be re-derived -- and because getting it backwards is the expensive
// mistake.
//
// The Linux build asks keyring for `linux-native-sync-persistent`, which reads
// like a backend swap but is only a feature selection. What that selection
// actually produces is the trap. In keyring 3.6.3:
//
//   - the module's own docs describe an entry holding *both* stores, keyutils
//     as a cache and secret-service for storage beyond reboot, and
//     `KeyutilsPersistentCredential::get_password` really does read keyutils
//     first;
//   - but `KeyutilsPersistentCredentialBuilder::build` -- the function
//     `Entry::new` actually reaches through the default builder -- returns
//     `SsCredential::new_with_target`. Not the two-store entry.
//
// So the documented design and the default construction path disagree, and the
// construction path is the one that runs. `Entry::new` consults the secret
// service and never touches the kernel keyring.
//
// That leaves the state this project introduced when it switched the feature:
// on a machine that upgraded without rebooting, the old key is still in the
// kernel keyring, the rows encrypted under it are still readable, and the
// secret service has no entry for it. Read through `Entry::new`, that key is
// invisible, and the next write mints a replacement over the top of it --
// after which those rows are gone.
//
// Hence `open_stores`: both credentials are constructed explicitly so the
// secret service is asked first (which is what the default path reads, so
// everything this build has written is already there) and the kernel keyring
// is reached only when the secret service has nothing. A key found that way is
// written through to the secret service -- the same key, so no ciphertext is
// re-encrypted and the old copy is left in place.
//
// What this does not do is prove the result, and the two halves need checking
// separately: "still readable in this session" is the kernel-keyring fallback,
// and "still readable after a reboot" is the write-through. Neither can be
// exercised without a real keyring, so the tests here drive the decision
// through the same seams the stores are reached by rather than pretending to
// cover the stores themselves.

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

/// One store's answer about the master key.
#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreProbe {
    /// A key is stored here and decoded to 32 bytes.
    Found([u8; 32]),
    /// This store holds nothing under our service and account.
    Absent,
    /// The store could not be built or read, for any reason other than being
    /// empty.
    Unavailable,
}

#[cfg(not(windows))]
fn probe_store(
    store: Option<&(dyn keyring::credential::CredentialApi + Send + Sync)>,
    label: &str,
) -> StoreProbe {
    let Some(store) = store else {
        return StoreProbe::Unavailable;
    };
    match store.get_password() {
        Ok(stored) => match decode_master_key(&stored) {
            Some(key) => StoreProbe::Found(key),
            None => {
                crate::warn!("[encryption] {label} holds a master key that is not 32 base64 bytes");
                StoreProbe::Unavailable
            }
        },
        Err(keyring::Error::NoEntry) => StoreProbe::Absent,
        Err(err) => {
            crate::warn!("[encryption] reading the master key from {label} failed: {err}");
            StoreProbe::Unavailable
        }
    }
}

/// Where a key came from, and whether anything durable now holds it.
#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyOrigin {
    /// Already in the persistent store. Nothing to do.
    Persistent,
    /// Recovered from the legacy kernel store and successfully written forward,
    /// so it now outlives this session.
    Migrated,
    /// Recovered from the legacy kernel store, but the write forward failed. The
    /// key can still decrypt what is already stored; it must not be used for
    /// anything new.
    Unpersisted,
}

/// A key, and whether a store that survives the session holds it.
///
/// The two answers are separate on purpose. Being able to *decrypt* a recovered
/// key is worth having even when nothing durable holds it, because that is how
/// existing rows stay readable. Being able to *encrypt* under it is not the
/// same bargain: a key that only exists for this session turns every row written
/// with it into a row that dies with the session, and no later retry can undo
/// that because the ciphertext has already been produced and stored.
#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedMasterKey {
    key: [u8; 32],
    persisted: bool,
}

/// The store that answers first, and the legacy one behind it.
///
/// Secret Service is asked first because it is what `Entry::new` reads, so
/// every key this build has written is already there. keyutils is only reached
/// when Secret Service has nothing -- which is exactly the state a machine that
/// upgraded without rebooting is in.
#[cfg(not(windows))]
fn open_stores() -> (
    Option<Box<dyn keyring::credential::CredentialApi + Send + Sync>>,
    Option<Box<dyn keyring::credential::CredentialApi + Send + Sync>>,
) {
    let persistent = keyring::secret_service::SsCredential::new_with_target(
        None,
        MASTER_KEY_SERVICE,
        MASTER_KEY_ACCOUNT,
    )
    .ok()
    .map(|credential| {
        Box::new(credential) as Box<dyn keyring::credential::CredentialApi + Send + Sync>
    });
    let legacy = keyring::keyutils::KeyutilsCredential::new_with_target(
        None,
        MASTER_KEY_SERVICE,
        MASTER_KEY_ACCOUNT,
    )
    .ok()
    .map(|credential| {
        Box::new(credential) as Box<dyn keyring::credential::CredentialApi + Send + Sync>
    });
    (persistent, legacy)
}

/// Finds the key, migrating a legacy one into the persistent store on the way.
///
/// The decision is a function of what the two stores say, not of what any of
/// them failed to answer, so it is written as one and tested through the same
/// seams the real stores are reached by.
#[cfg(not(windows))]
fn resolve_master_key(
    persistent: &Option<Box<dyn keyring::credential::CredentialApi + Send + Sync>>,
    legacy: &Option<Box<dyn keyring::credential::CredentialApi + Send + Sync>>,
) -> (
    MasterKeyRead,
    Option<ResolvedMasterKey>,
    Option<KeyOrigin>,
) {
    match probe_store(persistent.as_deref(), "the secret service") {
        StoreProbe::Found(key) => (
            MasterKeyRead::Present,
            Some(ResolvedMasterKey {
                key,
                persisted: true,
            }),
            Some(KeyOrigin::Persistent),
        ),
        StoreProbe::Unavailable => (MasterKeyRead::Unavailable, None, None),
        StoreProbe::Absent => match probe_store(legacy.as_deref(), "the kernel keyring") {
            StoreProbe::Absent => (MasterKeyRead::Missing, None, None),
            StoreProbe::Unavailable => (MasterKeyRead::Unavailable, None, None),
            StoreProbe::Found(key) => {
                // The same key, written through to the store that outlives the
                // session. No ciphertext is touched and the old copy is left
                // alone.
                let written = persistent.as_deref().is_some_and(|store| {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&key);
                    match store.set_password(&encoded) {
                        Ok(()) => true,
                        Err(err) => {
                            crate::warn!(
                                "[encryption] recovered the master key from the kernel keyring \
                                 but could not copy it to the secret service: {err}. Existing \
                                 records stay readable this session; new ones will not be written \
                                 until a store is available."
                            );
                            false
                        }
                    }
                });
                (
                    MasterKeyRead::Present,
                    Some(ResolvedMasterKey {
                        key,
                        persisted: written,
                    }),
                    Some(if written {
                        KeyOrigin::Migrated
                    } else {
                        KeyOrigin::Unpersisted
                    }),
                )
            }
        },
    }
}

#[cfg(not(windows))]
fn read_master_key() -> Result<ResolvedMasterKey, MasterKeyRead> {
    let (persistent, legacy) = open_stores();
    if persistent.is_none() && legacy.is_none() {
        crate::warn!("[encryption] no usable key store could be opened");
        return Err(MasterKeyRead::Unavailable);
    }
    let (read, resolved, _) = resolve_master_key(&persistent, &legacy);
    resolved.ok_or(read)
}

/// Cached only after a key has actually been decoded, so a keyring that was
/// briefly unavailable is asked again on the next call instead of being
/// remembered as "there is no key".
#[cfg(not(windows))]
static MASTER_KEY: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();

/// A key recovered from the legacy store that nothing durable holds yet.
///
/// It lives here so existing rows stay readable for this session, and it is
/// deliberately *not* the cache a writer consults: encrypting new rows under it
/// would produce ciphertext whose key dies with the session keyring, and no
/// later successful migration can rescue rows that were already written.
#[cfg(not(windows))]
static RECOVERED_KEY: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();

/// Two threads that both find nothing stored would otherwise each generate a
/// key, and whichever wrote last would strand everything encrypted under the
/// other's. Creation is not a hot path, so the plain lock costs nothing that
/// matters.
/// The whole read-decide-create-publish sequence happens under this one lock.
///
/// It used to be released before the cache was published, which left a window
/// exactly wide enough to destroy data: thread A created and stored K1, let go
/// of the lock, and had not yet published it; thread B took the lock, saw an
/// empty cache, did not re-read the store, and stored K2. B's publish then
/// failed silently because A had won, so B returned K2 while the process cached
/// K1 and the store held K2 -- the two threads that just ran now disagree, and
/// whatever the loser wrote is unreadable.
///
/// A process-level mutex still says nothing about two *processes* doing this at
/// once. That would need arbitration the kernel provides, which is out of scope
/// here; what this lock does guarantee is that one process never hands two
/// different keys to two callers.
#[cfg(not(windows))]
static MASTER_KEY_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Writes a fresh key to the store. The caller holds `MASTER_KEY_INIT_LOCK`.
///
/// The write is checked because a store that refuses it means there is nowhere
/// durable to keep the key. Handing one back anyway would encrypt with a key the
/// next login cannot read, which loses the data instead of leaving it
/// unencrypted.
///
/// Both stores get it, not just the persistent one. Writing only the secret
/// service would leave the kernel keyring empty, which is fine today and means
/// the read path has nothing to fall back to; writing both is what the
/// two-store design the feature selects was supposed to do in the first place.
/// A failure of the second store is not fatal once the first has taken the key.
#[cfg(not(windows))]
fn write_fresh_master_key() -> Option<[u8; 32]> {
    let (persistent, legacy) = open_stores();
    let persistent = persistent?;
    let mut key = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&key);
    if let Err(err) = persistent.set_password(&encoded) {
        crate::warn!("[encryption] could not store the master key: {}", err);
        return None;
    }
    if let Some(legacy) = legacy.as_deref() {
        if let Err(err) = legacy.set_password(&encoded) {
            crate::warn!(
                "[encryption] stored the master key, but not in the kernel keyring: {err}"
            );
        }
    }
    Some(key)
}

/// Reads, decides, creates if allowed, publishes, and returns the cache.
///
/// Cache and lock are parameters so the interleaving can be driven from a test:
/// production passes the process-wide pair, a test passes its own plus a store
/// that answers whatever it likes. `create` is `FnOnce` because it can only be
/// reached once per call -- after that the function has returned or published.
///
/// The value returned is whatever ended up in the cache, never a local copy.
/// That is what stops two callers from disagreeing when the second one loses
/// the publish.
#[cfg(not(windows))]
fn initialise_master_key(
    cache: &std::sync::OnceLock<[u8; 32]>,
    recovered: &std::sync::OnceLock<[u8; 32]>,
    lock: &std::sync::Mutex<()>,
    may_create: bool,
    for_writing: bool,
    read: impl Fn() -> Result<ResolvedMasterKey, MasterKeyRead>,
    create: impl FnOnce() -> Option<[u8; 32]>,
) -> Option<[u8; 32]> {
    if let Some(key) = cache.get() {
        return Some(*key);
    }
    // Only a reader looks here, and only when nothing durable has turned up.
    // A writer that did this would encrypt new rows under a key nothing else
    // holds, which is the failure this split exists to prevent.
    if !for_writing {
        if let Some(key) = recovered.get() {
            return Some(*key);
        }
    }

    let _guard = lock.lock().ok()?;
    // Whoever held the lock before us may have published in the meantime.
    if let Some(key) = cache.get() {
        return Some(*key);
    }

    let (read_result, decoded) = match read() {
        Ok(resolved) => (MasterKeyRead::Present, Some(resolved)),
        Err(reason) => (reason, None),
    };
    let resolved = match plan_master_key(read_result) {
        MasterKeyPlan::Use => decoded?,
        MasterKeyPlan::Create if may_create => ResolvedMasterKey {
            key: create()?,
            // A fresh key is only handed back after the store took it.
            persisted: true,
        },
        _ => return None,
    };

    if resolved.persisted {
        let _ = cache.set(resolved.key);
        return cache.get().copied();
    }

    // Recovered but not written anywhere durable. It can still read what is
    // already stored, so a reader keeps it -- in the recovery cache, which no
    // writer consults. A writer gets nothing, and because nothing was published
    // the next call runs this again, which is what makes the migration retry
    // once the store comes back instead of being skipped for the life of the
    // process.
    if !for_writing {
        let _ = recovered.set(resolved.key);
    }
    if for_writing {
        None
    } else {
        recovered.get().copied()
    }
}

/// The key used for writing. Returns `None` when the store cannot be read and
/// must not be written, which the callers turn into "leave the value as it is".
#[cfg(not(windows))]
fn master_key() -> Option<[u8; 32]> {
    initialise_master_key(
        &MASTER_KEY,
        &RECOVERED_KEY,
        &MASTER_KEY_INIT_LOCK,
        true,
        true,
        read_master_key,
        write_fresh_master_key,
    )
}

/// The key used for reading. It never creates one: decryption has to be able to
/// fail honestly rather than replace the key it is about to be measured
/// against.
#[cfg(not(windows))]
fn existing_master_key() -> Option<[u8; 32]> {
    initialise_master_key(
        &MASTER_KEY,
        &RECOVERED_KEY,
        &MASTER_KEY_INIT_LOCK,
        false,
        false,
        read_master_key,
        || None,
    )
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

    // The Linux path only, because on Windows the key comes from DPAPI and none
    // of this exists. `initialise_master_key` takes its cache and lock as
    // arguments precisely so a test can own them: the production ones are
    // process-wide statics, and a test sharing them would be racing every other
    // test in the binary.
    #[cfg(not(windows))]
    mod linux_key_init {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        use std::sync::{Arc, Barrier, Mutex, OnceLock};
        use std::cell::Cell;

        // A `Credential` whose state can be read back after it has been boxed
        // and handed over, which is what checking the migration needs. A plain
        // mock cannot do that -- it is moved into the box -- so this exists to
        // let a test assert that the key landed in the persistent store, not
        // merely that it was handed back.
        #[derive(Debug, Clone, Default)]
        struct KeyringStore(Arc<std::sync::Mutex<KeyringStoreState>>);

        #[derive(Debug, Default)]
        struct KeyringStoreState {
            stored: Option<Vec<u8>>,
            failure: Option<keyring::Error>,
            write_failure: Option<keyring::Error>,
            sticky_write_failure: bool,
            writes: usize,
        }

        impl KeyringStore {
            fn holding(key: [u8; 32]) -> Self {
                let store = Self::default();
                store.seed(key);
                store
            }

            fn failing(err: keyring::Error) -> Self {
                let store = Self::default();
                store.0.lock().expect("store").failure = Some(err);
                store
            }

            fn seed(&self, key: [u8; 32]) {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&key);
                self.0.lock().expect("store").stored = Some(encoded.into_bytes());
            }

            fn put_junk(&self) {
                self.0.lock().expect("store").stored = Some(b"not a key".to_vec());
            }

            fn fail_writes(&self, err: keyring::Error) {
                let mut guard = self.0.lock().expect("store");
                guard.write_failure = Some(err);
                guard.sticky_write_failure = true;
            }

            fn clear_write_failure(&self) {
                let mut guard = self.0.lock().expect("store");
                guard.write_failure = None;
                guard.sticky_write_failure = false;
            }

            fn fail_next_write(&self, err: keyring::Error) {
                // Only a write fails, so a test can have a store that reads
                // "empty" and still refuse to take a key -- which is the whole
                // shape of the bug this covers. A failure injected on read would
                // take the unavailable branch instead and prove nothing.
                self.0.lock().expect("store").write_failure = Some(err);
            }

            fn stored_key(&self) -> Option<[u8; 32]> {
                let guard = self.0.lock().expect("store");
                guard
                    .stored
                    .as_ref()
                    .and_then(|bytes| decode_master_key(std::str::from_utf8(bytes).ok()?))
            }

            fn writes(&self) -> usize {
                self.0.lock().expect("store").writes
            }
        }

        impl keyring::credential::CredentialApi for KeyringStore {
            fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
                let mut guard = self.0.lock().expect("store");
                if let Some(err) = guard.failure.take() {
                    return Err(err);
                }
                if guard.sticky_write_failure {
                    // Stays refused until told otherwise, which is what a
                    // locked or absent store actually looks like.
                    if let Some(err) = &guard.write_failure {
                        return Err(errring(err));
                    }
                }
                if let Some(err) = guard.write_failure.take() {
                    return Err(err);
                }
                guard.stored = Some(secret.to_vec());
                guard.writes += 1;
                Ok(())
            }

            fn get_secret(&self) -> keyring::Result<Vec<u8>> {
                let mut guard = self.0.lock().expect("store");
                if let Some(err) = guard.failure.take() {
                    return Err(err);
                }
                match &guard.stored {
                    Some(stored) => Ok(stored.clone()),
                    None => Err(keyring::Error::NoEntry),
                }
            }

            fn delete_credential(&self) -> keyring::Result<()> {
                self.0.lock().expect("store").stored = None;
                Ok(())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        fn resolve_with(
            persistent: KeyringStore,
            legacy: KeyringStore,
        ) -> (
            MasterKeyRead,
            Option<ResolvedMasterKey>,
            Option<KeyOrigin>,
        ) {
            let boxed = |store: &KeyringStore| {
                Some(Box::new(store.clone())
                    as Box<dyn keyring::credential::CredentialApi + Send + Sync>)
            };
            resolve_master_key(&boxed(&persistent), &boxed(&legacy))
        }

                /// A store that already holds its key durably, for the tests that drive
        /// initialise_master_key directly.
        fn persisted(key: [u8; 32]) -> ResolvedMasterKey {
            ResolvedMasterKey { key, persisted: true }
        }

                /// Reads through the real resolver, so the tests below drive the same
        /// path production takes rather than a stand-in for it.
        /// keyring::Error is not Clone, so a sticky failure is reported
        /// afresh each time by rebuilding the same kind of error.
        fn errring(err: &keyring::Error) -> keyring::Error {
            keyring::Error::PlatformFailure(Box::new(std::io::Error::other(
                err.to_string(),
            )))
        }

        fn boxed_stores(
            persistent: &KeyringStore,
            legacy: &KeyringStore,
        ) -> (
            Option<Box<dyn keyring::credential::CredentialApi + Send + Sync>>,
            Option<Box<dyn keyring::credential::CredentialApi + Send + Sync>>,
        ) {
            let boxed = |store: &KeyringStore| {
                Some(Box::new(store.clone())
                    as Box<dyn keyring::credential::CredentialApi + Send + Sync>)
            };
            (boxed(persistent), boxed(legacy))
        }

        fn read_through(
            persistent: KeyringStore,
            legacy: KeyringStore,
        ) -> impl Fn() -> Result<ResolvedMasterKey, MasterKeyRead> {
            move || {
                let (p, l) = boxed_stores(&persistent, &legacy);
                let (read, resolved, _) = resolve_master_key(&p, &l);
                resolved.ok_or(read)
            }
        }

        fn unreadable() -> keyring::Error {
            keyring::Error::PlatformFailure(Box::new(std::io::Error::other("store is locked")))
        }

        // The state the feature switch left behind, and the whole point of the
        // change: the persistent store has nothing, the kernel keyring still
        // holds the key every existing row is encrypted under.
        //
        // Reached through `Entry::new` that key is invisible, and the next
        // write mints a replacement over it. Here it must be found -- and since
        // a kernel keyring entry does not survive a reboot, it must also be
        // written through to the store that does.
        #[test]
        fn a_key_left_in_the_kernel_keyring_is_recovered_and_written_forward() {
            let persistent = KeyringStore::default();
            let legacy = KeyringStore::holding([7u8; 32]);

            let (read, key, origin) = resolve_with(persistent.clone(), legacy.clone());

            assert_eq!(read, MasterKeyRead::Present);
            assert_eq!(
                key.map(|resolved| resolved.key),
                Some([7u8; 32]),
                "the key that comes back must be the one the rows were written under"
            );
            assert_eq!(origin, Some(KeyOrigin::Migrated));
            assert_eq!(
                persistent.stored_key(),
                Some([7u8; 32]),
                "the key must be copied forward, or the next reboot loses it again"
            );
            assert_eq!(persistent.writes(), 1);
        }

        // Nothing to migrate: the secret service answers first because it is
        // what the default path reads, and its key wins without the legacy
        // store being consulted at all.
        #[test]
        fn a_key_already_in_the_persistent_store_is_used_untouched() {
            let persistent = KeyringStore::holding([1u8; 32]);
            let legacy = KeyringStore::holding([2u8; 32]);

            let (read, key, origin) = resolve_with(persistent.clone(), legacy.clone());

            assert_eq!(read, MasterKeyRead::Present);
            assert_eq!(key.map(|resolved| resolved.key), Some([1u8; 32]));
            assert_eq!(origin, Some(KeyOrigin::Persistent));
            assert_eq!(persistent.writes(), 0, "an existing key must not be rewritten");
            assert_eq!(legacy.writes(), 0, "the legacy store is not consulted as a rival");
        }

        // Both empty: nothing stored anywhere, which is the only state that
        // authorises creating a key.
        #[test]
        fn two_empty_stores_mean_no_key_yet() {
            let (read, key, origin) = resolve_with(KeyringStore::default(), KeyringStore::default());

            assert_eq!(read, MasterKeyRead::Missing);
            assert_eq!(key.map(|resolved| resolved.key), None);
            assert_eq!(origin, None);
        }

        // The data-loss case, and the reason the decision table is shaped the
        // way it is. The persistent store cannot be read, so we do not know
        // whether it holds a *different* key: answering with the legacy one
        // could encrypt under the wrong key, and answering "absent" would mint a
        // replacement. Neither is allowed.
        #[test]
        fn an_unreadable_persistent_store_is_never_treated_as_empty() {
            let persistent = KeyringStore::failing(unreadable());
            let legacy = KeyringStore::holding([3u8; 32]);

            let (read, key, origin) = resolve_with(persistent, legacy.clone());

            assert_eq!(read, MasterKeyRead::Unavailable);
            assert_eq!(key.map(|r| r.key), None, "the legacy key must not be used in its place");
            assert_eq!(origin, None);
            assert_eq!(legacy.writes(), 0, "and it must certainly not be written");
        }

        // The same, one level down: the persistent store is genuinely empty but
        // the legacy one cannot be reached. "I could not look" is not "there is
        // nothing there".
        #[test]
        fn an_unreadable_legacy_store_is_never_treated_as_empty() {
            let (read, key, _) = resolve_with(KeyringStore::default(), KeyringStore::failing(unreadable()));

            assert_eq!(read, MasterKeyRead::Unavailable);
            assert_eq!(key, None);
        }

        // A store that answers with something that is not a key is unreadable
        // rather than absent, for the same reason.
        #[test]
        fn a_store_holding_junk_is_not_read_as_empty() {
            let legacy = KeyringStore::default();
            legacy.put_junk();

            let (read, key, _) = resolve_with(KeyringStore::default(), legacy);

            assert_eq!(read, MasterKeyRead::Unavailable);
            assert_eq!(key, None);
        }

        // A store that cannot be opened at all is the same answer as one that
        // cannot be read: we do not know what is in it.
        #[test]
        fn a_store_that_could_not_be_opened_is_never_treated_as_empty() {
            let (persistent, legacy) = (
                None,
                Some(Box::new(KeyringStore::default()) as Box<dyn keyring::credential::CredentialApi + Send + Sync>),
            );

            let (read, key, _) = resolve_master_key(&persistent, &legacy);

            assert_eq!(read, MasterKeyRead::Unavailable);
            assert_eq!(key, None);
        }

        /// A store that answers "nothing stored" until something is written, and
        /// hands back whatever was written. It is what makes a second write
        /// detectable: each creator returns a *different* key, so the second one
        /// can only be wrong.
        struct FakeStore {
            stored: Mutex<Option<[u8; 32]>>,
            writes: AtomicUsize,
        }

        impl FakeStore {
            fn new() -> Arc<Self> {
                Arc::new(Self {
                    stored: Mutex::new(None),
                    writes: AtomicUsize::new(0),
                })
            }

            fn read(&self) -> Result<[u8; 32], MasterKeyRead> {
                match *self.stored.lock().expect("store") {
                    Some(key) => Ok(key),
                    None => Err(MasterKeyRead::Missing),
                }
            }

            fn write(&self, key: [u8; 32]) -> Option<[u8; 32]> {
                self.writes.fetch_add(1, AtomicOrdering::SeqCst);
                *self.stored.lock().expect("store") = Some(key);
                Some(key)
            }

            fn writes(&self) -> usize {
                self.writes.load(AtomicOrdering::SeqCst)
            }
        }

        // The regression, stated as something a test can actually decide.
        //
        // The bug was not that two callers might race -- a mutex can be held
        // across all of it -- it was *where* the read sat. Reading before
        // taking the lock means both callers get told "empty" and only then
        // race to create; reading inside means the loser never has to ask.
        //
        // A racing two-thread test cannot decide this: the scheduler may run
        // one caller to completion before the other starts, and the second then
        // never reads at all -- the broken code passes. So this asks the
        // question directly instead. `Mutex` is not reentrant, so `try_lock`
        // failing is unambiguous evidence that the lock is held -- by this very
        // thread, since the test is single-threaded.
        // The one the last review asked for, and the reason the two caches are
        // separate.
        //
        // The failure it guards against is a specific sequence: the secret
        // service reads empty, the kernel keyring holds K, and the write
        // forward fails. Treating that as an ordinary Present publishes K
        // into the write cache, after which master_key never reaches the
        // stores again for the life of the process -- so the migration is never
        // retried, and every row encrypted meanwhile rests on a key nothing
        // else holds.
        #[test]
        fn a_failed_write_forward_is_readable_but_never_writable() {
            let persistent = KeyringStore::default();
            let legacy = KeyringStore::holding([4u8; 32]);
            persistent.fail_writes(unreadable());

            let write_cache = OnceLock::new();
            let recovered_cache = OnceLock::new();
            let lock = Mutex::new(());

            // A reader gets the key: rows already stored must stay readable.
            let read_key = initialise_master_key(
                &write_cache,
                &recovered_cache,
                &lock,
                false,
                false,
                read_through(persistent.clone(), legacy.clone()),
                || panic!("a reader must never create"),
            );
            assert_eq!(
                read_key,
                Some([4u8; 32]),
                "recovering a key that failed to persist must still allow reading"
            );

            // A writer gets nothing, and nothing was published for writers.
            let write_key = {
                initialise_master_key(
                    &write_cache,
                    &recovered_cache,
                    &lock,
                    true,
                    true,
                    read_through(persistent.clone(), legacy.clone()),
                    || panic!("an existing key must never be replaced"),
                )
            };
            assert_eq!(
                write_key, None,
                "a key nothing durable holds must not be published for new writes"
            );
            assert!(
                write_cache.get().is_none(),
                "the write cache must stay empty, or every later write inherits this"
            );
        }

        // And the recovery half: because nothing was published, the next call
        // runs the whole thing again and the migration lands once the store
        // takes writes again.
        #[test]
        fn a_recovered_key_becomes_writable_once_the_store_takes_it() {
            let persistent = KeyringStore::default();
            let legacy = KeyringStore::holding([6u8; 32]);
            persistent.fail_next_write(unreadable());

            let write_cache = OnceLock::new();
            let recovered_cache = OnceLock::new();
            let lock = Mutex::new(());

            let first = initialise_master_key(
                &write_cache,
                &recovered_cache,
                &lock,
                true,
                true,
                read_through(persistent.clone(), legacy.clone()),
                || panic!("an existing key must never be replaced"),
            );
            assert_eq!(first, None, "the store still refuses writes");
            assert!(
                write_cache.get().is_none(),
                "and nothing was cached, so the next attempt really retries"
            );

            // The store is unlocked and takes writes now.
            let key = initialise_master_key(
                &write_cache,
                &recovered_cache,
                &lock,
                true,
                true,
                read_through(persistent.clone(), legacy.clone()),
                || panic!("an existing key must never be replaced"),
            );
            assert_eq!(key, Some([6u8; 32]), "the retry must land the key");
            assert_eq!(persistent.stored_key(), Some([6u8; 32]));
            assert_eq!(
                write_cache.get().copied(),
                Some([6u8; 32]),
                "only now is the key published for writing"
            );
        }

        #[test]
        fn the_store_is_read_while_the_init_lock_is_held() {
            let cache = OnceLock::new();
            let lock = Mutex::new(());
            let read_under_lock = Cell::new(false);

            let key = initialise_master_key(
                &cache,
                &OnceLock::new(),
                &lock,
                true,
                false,
                || {
                    if lock.try_lock().is_err() {
                        read_under_lock.set(true);
                    }
                    Err(MasterKeyRead::Missing)
                },
                || Some([1; 32]),
            );

            assert_eq!(key, Some([1; 32]));
            assert!(
                read_under_lock.get(),
                "the store must be read inside the critical section, otherwise two \
                 callers can both be told it is empty and both go on to create"
            );
        }

        // The same regression end to end, in the shape it actually damaged:
        // two callers, one store, each creator handing back a *different* key.
        //
        // This one states the invariant rather than proving the bug is gone --
        // that is the job of the test above. It would also have passed against
        // the old code, because whether the second caller wins the window
        // depends on scheduling.
        #[test]
        fn two_callers_racing_to_create_end_up_with_one_key() {
            let cache = OnceLock::new();
            let lock = Mutex::new(());
            let store = FakeStore::new();
            let start = Arc::new(Barrier::new(2));

            let keys: Vec<Option<[u8; 32]>> = std::thread::scope(|scope| {
                // References, so the `move` below copies rather than takes the
                // cache and lock the assertions still need afterwards.
                let shared_cache = &cache;
                let shared_lock = &lock;
                let handles: Vec<_> = (0..2u8)
                    .map(|id| {
                        let store = Arc::clone(&store);
                        let start = Arc::clone(&start);
                        scope.spawn(move || {
                            start.wait();
                            initialise_master_key(
                                shared_cache,
                                &OnceLock::new(),
                                shared_lock,
                                true,
                                true,
                                || store.read().map(persisted),
                                || store.write([id + 1; 32]),
                            )
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("caller thread"))
                    .collect()
            });

            assert!(
                keys.iter().all(Option::is_some),
                "an empty store authorises exactly one key for both callers: {keys:?}"
            );
            assert_eq!(
                keys[0], keys[1],
                "two callers must never be handed two different keys"
            );
            assert_eq!(
                store.writes(),
                1,
                "the store must be written once, not once per racing caller"
            );
            assert_eq!(
                cache.get().copied(),
                keys[0],
                "what a caller is handed must be what the process caches"
            );
        }

        // Decryption must be able to fail honestly. `may_create = false` is what
        // stops a read of a store that cannot answer from minting a key and
        // then "decrypting" everything to noise.
        #[test]
        fn a_reader_never_creates_a_key() {
            let cache = OnceLock::new();
            let lock = Mutex::new(());
            let store = FakeStore::new();

            let key = initialise_master_key(
                &cache,
                &OnceLock::new(),
                &lock,
                false,
                false,
                || store.read().map(persisted),
                || panic!("a reader must never reach the creator"),
            );

            assert_eq!(key, None, "nothing is stored, so there is nothing to read");
            assert_eq!(store.writes(), 0, "a reader must not write");
            assert!(cache.get().is_none(), "a reader must not publish either");
        }

        // A store that answers for some reason other than "empty" is the case
        // that destroyed data once. It has to stay unanswered *and* unwritten
        // even when this caller is the one allowed to create.
        #[test]
        fn an_unavailable_store_is_neither_written_nor_cached() {
            let cache = OnceLock::new();
            let lock = Mutex::new(());
            let store = FakeStore::new();

            let key = initialise_master_key(
                &cache,
                &OnceLock::new(),
                &lock,
                true,
                false,
                || Err(MasterKeyRead::Unavailable),
                || store.write([9; 32]),
            );

            assert_eq!(key, None);
            assert_eq!(store.writes(), 0, "an unavailable store must not be written");
            assert!(cache.get().is_none());
        }

        // A key that is already there is used as it is. Creating here is the
        // bug from the other side: it would overwrite a working key over rows
        // encrypted under it.
        #[test]
        fn a_present_key_is_used_without_being_rewritten() {
            let cache = OnceLock::new();
            let lock = Mutex::new(());
            let store = FakeStore::new();
            store.write([5; 32]).expect("seed");

            let key = initialise_master_key(
                &cache,
                &OnceLock::new(),
                &lock,
                true,
                false,
                || store.read().map(persisted),
                || panic!("a present key must never reach the creator"),
            );

            assert_eq!(key, Some([5; 32]));
            assert_eq!(store.writes(), 1, "only the seeding write");
        }
    }
}
