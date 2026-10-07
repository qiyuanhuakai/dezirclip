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

// Why there is no migration away from an older keyring backend, in enough
// detail that it does not have to be re-derived -- or "fixed" in the other
// direction by adding one that would strand the keys it meant to rescue.
//
// The Linux build asks keyring for `linux-native-sync-persistent`. That feature
// name reads like a backend swap and has been mistaken for one, so here is what
// it actually selects (keyring 3.6.3, the locked version):
//
//   - the crate documents the feature as using *both* `keyutils` and
//     `sync-secret-service`, with keyutils as the cache "available to headless
//     processes" and secret-service as the "credential storage beyond reboot";
//   - `keyutils_persistent::KeyutilsPersistentCredential::get_password` reads
//     `self.keyutils` first and only falls back to `self.ss` on a miss, caching
//     a secret-service hit back into keyutils;
//   - `set_password` writes to both.
//
// So the chain got *wider*, not different. A key left in keyutils by an older
// build is still the first thing read, which is why an upgrade does not need to
// move it anywhere -- and why "the old key was never persistent, so there is
// nothing to migrate" is the wrong reason to skip the question: the key is
// still there on a machine that has not rebooted, and the correct action was
// always to stop treating "I could not read it" as "there is none".
//
// What this depends on is a third party's internal read order, under a `3`
// constraint that allows upgrades. That is worth re-checking when keyring is
// next bumped, and it is why the invariant is written down here rather than
// left implicit. It cannot be pinned by a unit test: proving it needs a real
// session keyring and a secret service to survive a reboot against.

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
#[cfg(not(windows))]
fn write_fresh_master_key() -> Option<[u8; 32]> {
    let entry = keyring::Entry::new(MASTER_KEY_SERVICE, MASTER_KEY_ACCOUNT).ok()?;
    let mut key = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&key);
    if let Err(err) = entry.set_password(&encoded) {
        crate::warn!("[encryption] could not store the master key: {}", err);
        return None;
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
    lock: &std::sync::Mutex<()>,
    may_create: bool,
    read: impl Fn() -> Result<[u8; 32], MasterKeyRead>,
    create: impl FnOnce() -> Option<[u8; 32]>,
) -> Option<[u8; 32]> {
    if let Some(key) = cache.get() {
        return Some(*key);
    }

    let _guard = lock.lock().ok()?;
    // Whoever held the lock before us may have published in the meantime.
    if let Some(key) = cache.get() {
        return Some(*key);
    }

    let (read_result, decoded) = match read() {
        Ok(key) => (MasterKeyRead::Present, Some(key)),
        Err(reason) => (reason, None),
    };
    let key = match plan_master_key(read_result) {
        MasterKeyPlan::Use => decoded?,
        MasterKeyPlan::Create if may_create => create()?,
        _ => return None,
    };
    let _ = cache.set(key);
    cache.get().copied()
}

/// The key used for writing. Returns `None` when the store cannot be read and
/// must not be written, which the callers turn into "leave the value as it is".
#[cfg(not(windows))]
fn master_key() -> Option<[u8; 32]> {
    initialise_master_key(
        &MASTER_KEY,
        &MASTER_KEY_INIT_LOCK,
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
    initialise_master_key(&MASTER_KEY, &MASTER_KEY_INIT_LOCK, false, read_master_key, || None)
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
        #[test]
        fn the_store_is_read_while_the_init_lock_is_held() {
            let cache = OnceLock::new();
            let lock = Mutex::new(());
            let read_under_lock = Cell::new(false);

            let key = initialise_master_key(
                &cache,
                &lock,
                true,
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
                                shared_lock,
                                true,
                                || store.read(),
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
                &lock,
                false,
                || store.read(),
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
                &lock,
                true,
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
                &lock,
                true,
                || store.read(),
                || panic!("a present key must never reach the creator"),
            );

            assert_eq!(key, Some([5; 32]));
            assert_eq!(store.writes(), 1, "only the seeding write");
        }
    }
}
