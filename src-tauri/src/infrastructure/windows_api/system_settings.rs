//! Changes to Windows settings carry an exact-value, write-ahead restoration receipt.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::sync::{Mutex, OnceLock};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, HANDLE, HLOCAL, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, OpenProcessToken, ReleaseMutex, WaitForSingleObject,
};
use winreg::enums::*;
use winreg::{RegKey, RegValue};

const OWNERSHIP_KEY: &str = r"Software\io.github.qiyuanhuakai.dezirclip\SystemSettingsOwnership\v1";
const CLIPBOARD_KEY: &str = r"Software\Microsoft\Clipboard";
const EXPLORER_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const RESTORE_ARGUMENT: &str = "--restore-system-settings";

#[derive(Clone, Copy)]
enum Setting {
    History,
    Cloud,
    WinV,
}

impl Setting {
    fn index(self) -> usize {
        match self { Self::History => 0, Self::Cloud => 1, Self::WinV => 2 }
    }

    fn key(self) -> &'static str {
        match self {
            Self::History | Self::Cloud => CLIPBOARD_KEY,
            Self::WinV => EXPLORER_KEY,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::History => "EnableClipboardHistory",
            Self::Cloud => "EnableCloudClipboard",
            Self::WinV => "DisabledHotkeys",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawValue {
    value_type: u32,
    bytes: Vec<u8>,
}

impl RawValue {
    fn from_registry(value: RegValue) -> Self {
        Self {
            value_type: value.vtype as u32,
            bytes: value.bytes,
        }
    }

    fn to_registry(&self) -> io::Result<RegValue> {
        let vtype = match self.value_type {
            0 => REG_NONE,
            1 => REG_SZ,
            2 => REG_EXPAND_SZ,
            3 => REG_BINARY,
            4 => REG_DWORD,
            5 => REG_DWORD_BIG_ENDIAN,
            6 => REG_LINK,
            7 => REG_MULTI_SZ,
            8 => REG_RESOURCE_LIST,
            9 => REG_FULL_RESOURCE_DESCRIPTOR,
            10 => REG_RESOURCE_REQUIREMENTS_LIST,
            11 => REG_QWORD,
            _ => return Err(invalid_receipt()),
        };
        Ok(RegValue {
            vtype,
            bytes: self.bytes.clone(),
        })
    }

    fn dword(value: u32) -> Self {
        Self {
            value_type: REG_DWORD as u32,
            bytes: value.to_le_bytes().to_vec(),
        }
    }

    fn string(value: &str) -> Self {
        Self {
            value_type: REG_SZ as u32,
            bytes: value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
    }

    fn as_string(&self) -> io::Result<String> {
        if self.value_type != REG_SZ as u32 || self.bytes.len() % 2 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "DisabledHotkeys is not a valid REG_SZ",
            ));
        }
        let mut units: Vec<u16> = self
            .bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        if units.pop() != Some(0) || units.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "DisabledHotkeys is not a terminated REG_SZ",
            ));
        }
        String::from_utf16(&units).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "DisabledHotkeys is not valid UTF-16",
            )
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    original: Option<RawValue>,
    expected: RawValue,
}

fn invalid_receipt() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Windows setting restoration receipt is invalid",
    )
}

trait SettingsStore {
    fn current(&self, setting: Setting) -> io::Result<Option<RawValue>>;
    fn write_current(&mut self, setting: Setting, value: Option<&RawValue>) -> io::Result<()>;
    fn receipt(&self, setting: Setting) -> io::Result<Option<Receipt>>;
    fn write_receipt(&mut self, setting: Setting, receipt: Option<&Receipt>) -> io::Result<()>;
}

fn apply_value(
    store: &mut impl SettingsStore,
    setting: Setting,
    value: RawValue,
) -> io::Result<bool> {
    let current = store.current(setting)?;
    if store.receipt(setting)?.is_some() {
        // An override is released before it can be replaced. In particular,
        // startup never reasserts an override after a later user or policy change.
        return Ok(false);
    }
    if current.as_ref() == Some(&value) {
        return Ok(false);
    }
    let receipt = Receipt {
        version: 1,
        original: current,
        expected: value.clone(),
    };
    store.write_receipt(setting, Some(&receipt))?;
    store.write_current(setting, Some(&value))?;
    Ok(true)
}

fn restore_value(store: &mut impl SettingsStore, setting: Setting) -> io::Result<bool> {
    let Some(receipt) = store.receipt(setting)? else {
        return Ok(false);
    };
    let current = store.current(setting)?;
    let changed = current.as_ref() == Some(&receipt.expected) && current != receipt.original;
    if changed {
        store.write_current(setting, receipt.original.as_ref())?;
    }
    store.write_receipt(setting, None)?;
    Ok(changed)
}

fn restore_all(store: &mut impl SettingsStore) -> io::Result<bool> {
    let mut changed = false;
    for setting in [Setting::History, Setting::Cloud, Setting::WinV] {
        changed |= restore_value(store, setting)?;
    }
    Ok(changed)
}

fn value_after_release(
    store: &impl SettingsStore,
    setting: Setting,
) -> io::Result<Option<RawValue>> {
    let current = store.current(setting)?;
    Ok(match store.receipt(setting)? {
        Some(receipt) if current.as_ref() == Some(&receipt.expected) => receipt.original,
        _ => current,
    })
}

fn release_history(store: &mut impl SettingsStore) -> io::Result<bool> {
    for setting in [Setting::History, Setting::Cloud] {
        if value_after_release(store, setting)?.as_ref() == Some(&RawValue::dword(0)) {
            return Err(io::Error::other(format!(
                "{} remains disabled by a pre-existing or externally changed value. Review Windows clipboard settings and enable it manually; DezirClip cannot safely guess the original value.",
                setting.name()
            )));
        }
    }
    let history = restore_value(store, Setting::History)?;
    let cloud = restore_value(store, Setting::Cloud)?;
    Ok(history || cloud)
}

fn release_win_v(store: &mut impl SettingsStore) -> io::Result<bool> {
    let hotkeys = value_after_release(store, Setting::WinV)?
        .map(|value| value.as_string())
        .transpose()?
        .unwrap_or_default();
    if hotkeys.contains(['V', 'v']) {
        return Err(io::Error::other(
            "Win+V remains disabled by an unowned or externally changed DisabledHotkeys value. Review HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced\\DisabledHotkeys and restore Win+V manually before turning this option off. DezirClip has no trustworthy original value."
        ));
    }
    restore_all(store)
}

fn disable_history(store: &mut impl SettingsStore) -> io::Result<bool> {
    let history = apply_value(store, Setting::History, RawValue::dword(0))?;
    let cloud = apply_value(store, Setting::Cloud, RawValue::dword(0))?;
    Ok(history || cloud)
}

fn optimize_win_v(store: &mut impl SettingsStore) -> io::Result<bool> {
    let current = store.current(Setting::WinV)?;
    let hotkeys = current
        .map(|value| value.as_string())
        .transpose()?
        .unwrap_or_default();
    let mut changed = false;
    if !hotkeys.contains(['V', 'v']) {
        changed |= apply_value(
            store,
            Setting::WinV,
            RawValue::string(&format!("{hotkeys}V")),
        )?;
    }
    changed |= disable_history(store)?;
    Ok(changed)
}

#[derive(Clone, PartialEq, Eq)]
struct SettingState {
    value: Option<RawValue>,
    receipt: Option<Receipt>,
}

#[derive(Clone)]
struct SettingWrites {
    before: SettingState,
    expected: SettingState,
    value_written: bool,
    receipt_written: bool,
}

#[derive(Clone, Default)]
struct SettingsChange {
    writes: [Option<SettingWrites>; 3],
}

const SETTINGS: [Setting; 3] = [Setting::History, Setting::Cloud, Setting::WinV];

fn read_state(store: &impl SettingsStore, setting: Setting) -> io::Result<SettingState> {
    Ok(SettingState {
        value: store.current(setting)?,
        receipt: store.receipt(setting)?,
    })
}

#[cfg(test)]
fn snapshot(store: &impl SettingsStore) -> io::Result<[SettingState; 3]> {
    Ok([
        read_state(store, Setting::History)?,
        read_state(store, Setting::Cloud)?,
        read_state(store, Setting::WinV)?,
    ])
}

struct RecordingStore<'a, S> {
    store: &'a mut S,
    change: SettingsChange,
}

impl<S: SettingsStore> RecordingStore<'_, S> {
    fn writes(&mut self, setting: Setting, observed: &SettingState) -> &mut SettingWrites {
        self.change.writes[setting.index()].get_or_insert_with(|| SettingWrites {
            before: observed.clone(), expected: observed.clone(),
            value_written: false, receipt_written: false,
        })
    }
}

impl<S: SettingsStore> SettingsStore for RecordingStore<'_, S> {
    fn current(&self, setting: Setting) -> io::Result<Option<RawValue>> {
        self.store.current(setting)
    }

    fn receipt(&self, setting: Setting) -> io::Result<Option<Receipt>> {
        self.store.receipt(setting)
    }

    fn write_current(&mut self, setting: Setting, value: Option<&RawValue>) -> io::Result<()> {
        let observed = read_state(self.store, setting)?;
        self.store.write_current(setting, value)?;
        // Only a successful write claims this field, and its expected state is
        // the requested bytes, never a later read that could belong to another writer.
        let writes = self.writes(setting, &observed);
        if !writes.value_written { writes.before.value = observed.value; }
        writes.value_written = true;
        writes.expected.value = value.cloned();
        Ok(())
    }

    fn write_receipt(&mut self, setting: Setting, receipt: Option<&Receipt>) -> io::Result<()> {
        let observed = read_state(self.store, setting)?;
        self.store.write_receipt(setting, receipt)?;
        let writes = self.writes(setting, &observed);
        if !writes.receipt_written { writes.before.receipt = observed.receipt; }
        writes.receipt_written = true;
        writes.expected.receipt = receipt.cloned();
        Ok(())
    }
}

fn rollback_change(store: &mut impl SettingsStore, change: &mut SettingsChange) -> io::Result<bool> {
    let mut changed = false;
    for setting in SETTINGS {
        let Some(writes) = change.writes[setting.index()].as_mut() else { continue; };
        let before = writes.before.clone();
        let expected = writes.expected.clone();
        if before == expected || read_state(store, setting)? != expected {
            continue;
        }
        // Restoring an old receipt precedes its value; releasing a new receipt
        // follows its value. Interrupted rollback therefore retains recovery evidence.
        if writes.receipt_written && before.receipt.is_some() && before.receipt != expected.receipt {
            store.write_receipt(setting, before.receipt.as_ref())?;
            writes.expected.receipt = before.receipt.clone();
        }
        if writes.value_written && before.value != expected.value {
            store.write_current(setting, before.value.as_ref())?;
            writes.expected.value = before.value.clone();
            changed = true;
        }
        if writes.receipt_written && before.receipt.is_none() && before.receipt != expected.receipt {
            store.write_receipt(setting, None)?;
            writes.expected.receipt = None;
        }
    }
    Ok(changed)
}

fn stage_change(
    store: &mut impl SettingsStore,
    enabled: bool,
) -> io::Result<(bool, SettingsChange)> {
    let (result, mut change) = {
        let mut recording = RecordingStore { store, change: SettingsChange::default() };
        let result = if enabled {
            optimize_win_v(&mut recording)
        } else {
            release_win_v(&mut recording)
        };
        (result, recording.change)
    };
    match result {
        Ok(changed) => Ok((changed, change)),
        Err(error) => {
            rollback_change(store, &mut change)?;
            Err(error)
        }
    }
}

fn pending_changes() -> &'static Mutex<HashMap<String, SettingsChange>> {
    static PENDING: OnceLock<Mutex<HashMap<String, SettingsChange>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

struct RegistryStore<'a> {
    root: &'a RegKey,
    prefix: &'a str,
}

impl RegistryStore<'_> {
    fn path(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_owned()
        } else {
            format!(r"{}\{}", self.prefix, key)
        }
    }

    fn read(&self, key: &str, name: &str) -> io::Result<Option<RegValue>> {
        let key = match self.root.open_subkey_with_flags(self.path(key), KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        match key.get_raw_value(name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write(&self, key: &str, name: &str, value: Option<RegValue>) -> io::Result<()> {
        if let Some(value) = value {
            let (key, _) = self
                .root
                .create_subkey_with_flags(self.path(key), KEY_QUERY_VALUE | KEY_SET_VALUE)?;
            key.set_raw_value(name, &value)
        } else {
            let key = match self
                .root
                .open_subkey_with_flags(self.path(key), KEY_SET_VALUE)
            {
                Ok(key) => key,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
            };
            match key.delete_value(name) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                result => result,
            }
        }
    }
}

impl SettingsStore for RegistryStore<'_> {
    fn current(&self, setting: Setting) -> io::Result<Option<RawValue>> {
        Ok(self
            .read(setting.key(), setting.name())?
            .map(RawValue::from_registry))
    }

    fn write_current(&mut self, setting: Setting, value: Option<&RawValue>) -> io::Result<()> {
        self.write(
            setting.key(),
            setting.name(),
            value.map(RawValue::to_registry).transpose()?,
        )
    }

    fn receipt(&self, setting: Setting) -> io::Result<Option<Receipt>> {
        let Some(raw) = self.read(OWNERSHIP_KEY, setting.name())? else {
            return Ok(None);
        };
        if raw.vtype != REG_BINARY {
            return Err(invalid_receipt());
        }
        let receipt: Receipt = serde_json::from_slice(&raw.bytes).map_err(|_| invalid_receipt())?;
        if receipt.version != 1 {
            return Err(invalid_receipt());
        }
        receipt.expected.to_registry()?;
        if let Some(original) = &receipt.original {
            original.to_registry()?;
        }
        Ok(Some(receipt))
    }

    fn write_receipt(&mut self, setting: Setting, receipt: Option<&Receipt>) -> io::Result<()> {
        let value = receipt
            .map(|receipt| {
                serde_json::to_vec(receipt)
                    .map(|bytes| RegValue {
                        bytes,
                        vtype: REG_BINARY,
                    })
                    .map_err(|_| invalid_receipt())
            })
            .transpose()?;
        self.write(OWNERSHIP_KEY, setting.name(), value)
    }
}

struct LocalAllocation(HLOCAL);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(self.0));
        }
    }
}

fn current_user_sid() -> io::Result<String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(io::Error::other)?;
    let result = (|| {
        let mut required = 0;
        unsafe {
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut required);
        }
        if required == 0 {
            return Err(io::Error::last_os_error());
        }
        // Pointer-width allocation keeps TOKEN_USER aligned for the native API.
        let mut buffer = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                required,
                &mut required,
            )
        }
        .map_err(io::Error::other)?;
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut text = PWSTR::null();
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(io::Error::other)?;
        let _allocation = LocalAllocation(HLOCAL(text.0.cast()));
        unsafe { text.to_string() }.map_err(io::Error::other)
    })();
    unsafe {
        let _ = CloseHandle(token);
    }
    result
}

fn settings_lock_name(sid: &str) -> String {
    format!("Global\\DezirClip.SystemSettingsOwnership.v1.{sid}")
}

struct SettingsLock(HANDLE);

impl SettingsLock {
    fn acquire() -> io::Result<Self> {
        let sid = current_user_sid()?;
        let name: Vec<u16> = settings_lock_name(&sid)
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let security: Vec<u16> = format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(security.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(io::Error::other)?;
        let _allocation = LocalAllocation(HLOCAL(descriptor.0));
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        // One same-user lock covers HKCU across sessions; the DACL excludes other users.
        let handle = unsafe { CreateMutexW(Some(&attributes), false, PCWSTR(name.as_ptr())) }
            .map_err(io::Error::other)?;
        let result = unsafe { WaitForSingleObject(handle, 5_000) };
        if result == WAIT_OBJECT_0 || result == WAIT_ABANDONED {
            Ok(Self(handle))
        } else {
            let error = if result == WAIT_TIMEOUT {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows settings are being changed by another process",
                )
            } else {
                io::Error::last_os_error()
            };
            unsafe {
                let _ = CloseHandle(handle);
            }
            Err(error)
        }
    }
}

impl Drop for SettingsLock {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

fn with_registry<T>(
    operation: impl FnOnce(&mut RegistryStore<'_>) -> io::Result<T>,
) -> io::Result<T> {
    let _guard = SettingsLock::acquire()?;
    let root = RegKey::predef(HKEY_CURRENT_USER);
    operation(&mut RegistryStore {
        root: &root,
        prefix: "",
    })
}

pub fn set_clipboard_history(enabled: bool) -> io::Result<()> {
    with_registry(|store| {
        if enabled {
            release_history(store)
        } else {
            disable_history(store)
        }
    })
    .map(|_| ())
}

pub fn set_win_v_optimization(enabled: bool) -> io::Result<bool> {
    with_registry(|store| {
        if enabled {
            optimize_win_v(store)
        } else {
            release_win_v(store)
        }
    })
}

/// GUI changes retain a private, bounded successful-write journal until confirmation.
pub fn begin_win_v_change(enabled: bool) -> io::Result<(bool, String)> {
    with_registry(|store| {
        let mut pending = pending_changes()
            .lock()
            .map_err(|_| io::Error::other("Windows settings change state is unavailable"))?;
        if pending.len() >= 16 {
            return Err(io::Error::other(
                "Too many pending Windows settings changes; finish the open setting dialogs first",
            ));
        }
        let (changed, change) = stage_change(store, enabled)?;
        let token = uuid::Uuid::new_v4().to_string();
        pending.insert(token.clone(), change);
        Ok((changed, token))
    })
}

pub fn finish_win_v_change(token: &str, rollback: bool) -> io::Result<bool> {
    with_registry(|store| {
        let mut pending = pending_changes()
            .lock()
            .map_err(|_| io::Error::other("Windows settings change state is unavailable"))?;
        let change = pending.get_mut(token).ok_or_else(|| {
            io::Error::other("Windows settings change token is missing or already finished")
        })?;
        let changed = if rollback {
            rollback_change(store, change)?
        } else {
            false
        };
        pending.remove(token);
        Ok(changed)
    })
}

pub fn win_v_optimized() -> io::Result<bool> {
    with_registry(|store| {
        Ok(store
            .current(Setting::WinV)?
            .map(|value| value.as_string())
            .transpose()?
            .is_some_and(|value| value.contains(['V', 'v'])))
    })
}

/// Runs before Tauri, logging, database access, and single-instance forwarding.
/// The NSIS pre-uninstall hook waits for this process before deleting the executable.
pub fn run_maintenance_command() -> Option<i32> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(RESTORE_ARGUMENT)) {
        return None;
    }
    if arguments.next().is_some() {
        return Some(2);
    }
    Some(match with_registry(|store| restore_all(store)) {
        Ok(_) => 0,
        Err(_) => 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MemoryStore {
        values: BTreeMap<&'static str, RawValue>,
        receipts: BTreeMap<&'static str, Receipt>,
    }

    impl SettingsStore for MemoryStore {
        fn current(&self, setting: Setting) -> io::Result<Option<RawValue>> {
            Ok(self.values.get(setting.name()).cloned())
        }
        fn write_current(&mut self, setting: Setting, value: Option<&RawValue>) -> io::Result<()> {
            if let Some(value) = value {
                self.values.insert(setting.name(), value.clone());
            } else {
                self.values.remove(setting.name());
            }
            Ok(())
        }
        fn receipt(&self, setting: Setting) -> io::Result<Option<Receipt>> {
            Ok(self.receipts.get(setting.name()).cloned())
        }
        fn write_receipt(&mut self, setting: Setting, value: Option<&Receipt>) -> io::Result<()> {
            if let Some(value) = value {
                self.receipts.insert(setting.name(), value.clone());
            } else {
                self.receipts.remove(setting.name());
            }
            Ok(())
        }
    }

    #[test]
    fn process_lock_can_be_released_and_reacquired_without_changing_settings() {
        drop(SettingsLock::acquire().unwrap());
        drop(SettingsLock::acquire().unwrap());
    }

    #[test]
    fn absent_values_and_hotkey_case_are_restored_exactly() {
        let mut store = MemoryStore::default();
        let hotkeys = RawValue::string("aZ");
        store.values.insert(Setting::WinV.name(), hotkeys.clone());
        assert!(optimize_win_v(&mut store).unwrap());
        assert_eq!(
            store.values[Setting::WinV.name()].as_string().unwrap(),
            "aZV"
        );
        assert!(restore_all(&mut store).unwrap());
        assert_eq!(store.values.len(), 1);
        assert_eq!(store.values[Setting::WinV.name()], hotkeys);
        assert!(store.receipts.is_empty());
    }

    #[test]
    fn original_registry_type_and_raw_data_survive_changes() {
        let mut store = MemoryStore::default();
        let original = RawValue {
            value_type: REG_BINARY as u32,
            bytes: vec![0, 255, 0, 1],
        };
        store
            .values
            .insert(Setting::History.name(), original.clone());
        assert!(disable_history(&mut store).unwrap());
        assert!(!disable_history(&mut store).unwrap());
        restore_all(&mut store).unwrap();
        assert_eq!(store.values[Setting::History.name()], original);
    }

    #[test]
    fn later_user_changes_are_preserved_on_startup_disable_and_uninstall() {
        let mut store = MemoryStore::default();
        optimize_win_v(&mut store).unwrap();
        let user_hotkeys = RawValue::string("X");
        store
            .values
            .insert(Setting::WinV.name(), user_hotkeys.clone());
        store
            .values
            .insert(Setting::History.name(), RawValue::dword(1));
        assert!(!optimize_win_v(&mut store).unwrap());
        assert_eq!(store.values[Setting::WinV.name()], user_hotkeys);
        restore_all(&mut store).unwrap();
        assert_eq!(store.values[Setting::WinV.name()], user_hotkeys);
        assert_eq!(store.values[Setting::History.name()], RawValue::dword(1));
        assert!(store.receipts.is_empty());
    }

    #[test]
    fn existing_settings_are_not_claimed_or_reset_without_a_receipt() {
        let mut store = MemoryStore::default();
        let hotkeys = RawValue::string("vA");
        store.values.insert(Setting::WinV.name(), hotkeys.clone());
        store
            .values
            .insert(Setting::History.name(), RawValue::dword(0));
        store
            .values
            .insert(Setting::Cloud.name(), RawValue::dword(0));
        assert!(!optimize_win_v(&mut store).unwrap());
        assert!(store.receipts.is_empty());
        assert!(!restore_all(&mut store).unwrap());
        assert_eq!(store.values[Setting::WinV.name()], hotkeys);
        assert_eq!(store.values.len(), 3);
    }

    #[test]
    fn interactive_release_reports_unowned_legacy_overrides_without_guessing() {
        let mut store = MemoryStore::default();
        store
            .values
            .insert(Setting::WinV.name(), RawValue::string("vA"));
        store
            .values
            .insert(Setting::History.name(), RawValue::dword(0));
        assert!(release_win_v(&mut store)
            .unwrap_err()
            .to_string()
            .contains("manually"));
        assert!(release_history(&mut store)
            .unwrap_err()
            .to_string()
            .contains("manually"));
        assert!(!restore_all(&mut store).unwrap());
        assert_eq!(store.values[Setting::WinV.name()], RawValue::string("vA"));
        assert_eq!(store.values[Setting::History.name()], RawValue::dword(0));
    }

    #[test]
    fn releasing_history_leaves_the_win_v_control_domain_unchanged() {
        let mut store = MemoryStore::default();
        store
            .values
            .insert(Setting::History.name(), RawValue::dword(1));
        optimize_win_v(&mut store).unwrap();
        assert!(release_history(&mut store).unwrap());
        assert_eq!(store.values[Setting::History.name()], RawValue::dword(1));
        assert_eq!(store.values[Setting::WinV.name()], RawValue::string("V"));
        assert!(store.receipt(Setting::WinV).unwrap().is_some());
        assert!(release_win_v(&mut store).unwrap());
        assert!(!store.values.contains_key(Setting::WinV.name()));
    }

    #[test]
    fn interactive_release_does_not_claim_success_after_external_disable() {
        let mut store = MemoryStore::default();
        optimize_win_v(&mut store).unwrap();
        store
            .values
            .insert(Setting::WinV.name(), RawValue::string("XV"));
        assert!(release_win_v(&mut store).is_err());
        assert!(store.receipt(Setting::WinV).unwrap().is_some());
        assert_eq!(store.values[Setting::WinV.name()], RawValue::string("XV"));
        restore_all(&mut store).unwrap();
        assert_eq!(store.values[Setting::WinV.name()], RawValue::string("XV"));
    }

    #[test]
    fn canceling_enable_restores_only_that_change_and_preserves_unowned_v() {
        let mut store = MemoryStore::default();
        store
            .values
            .insert(Setting::WinV.name(), RawValue::string("V"));
        store
            .values
            .insert(Setting::History.name(), RawValue::dword(1));
        store
            .values
            .insert(Setting::Cloud.name(), RawValue::dword(1));
        let before = snapshot(&store).unwrap();
        let (changed, mut change) = stage_change(&mut store, true).unwrap();
        assert!(changed);
        assert_eq!(store.values[Setting::History.name()], RawValue::dword(0));
        rollback_change(&mut store, &mut change).unwrap();
        assert!(snapshot(&store).unwrap() == before);
        assert!(store.receipts.is_empty());
    }

    #[test]
    fn rejected_disable_makes_no_changes_and_needs_no_inverse_optimization() {
        let mut store = MemoryStore::default();
        store
            .values
            .insert(Setting::WinV.name(), RawValue::string("V"));
        store
            .values
            .insert(Setting::History.name(), RawValue::dword(1));
        store
            .values
            .insert(Setting::Cloud.name(), RawValue::dword(1));
        let before = snapshot(&store).unwrap();
        assert!(stage_change(&mut store, false).is_err());
        assert!(snapshot(&store).unwrap() == before);
        assert!(store.receipts.is_empty());
    }

    #[test]
    fn rollback_preserves_preexisting_ownership_and_newer_external_changes() {
        let mut store = MemoryStore::default();
        store
            .values
            .insert(Setting::History.name(), RawValue::dword(1));
        apply_value(&mut store, Setting::History, RawValue::dword(0)).unwrap();
        let owned_history = read_state(&store, Setting::History).unwrap();
        store
            .values
            .insert(Setting::Cloud.name(), RawValue::dword(1));
        let (_, mut change) = stage_change(&mut store, true).unwrap();
        store
            .values
            .insert(Setting::Cloud.name(), RawValue::dword(2));
        rollback_change(&mut store, &mut change).unwrap();
        assert!(read_state(&store, Setting::History).unwrap() == owned_history);
        assert_eq!(store.values[Setting::Cloud.name()], RawValue::dword(2));
        assert!(!store.values.contains_key(Setting::WinV.name()));
    }

    #[test]
    fn canceling_disable_restores_the_original_active_receipts() {
        let mut store = MemoryStore::default();
        optimize_win_v(&mut store).unwrap();
        let before = snapshot(&store).unwrap();
        let (_, mut change) = stage_change(&mut store, false).unwrap();
        assert!(store.receipts.is_empty());
        rollback_change(&mut store, &mut change).unwrap();
        assert!(snapshot(&store).unwrap() == before);
    }

    struct DuringBeginStore {
        inner: MemoryStore,
        inject_on: Setting,
        inject: Option<fn(&mut MemoryStore)>,
        fail_cloud: bool,
        value_writes: Vec<&'static str>,
    }

    impl DuringBeginStore {
        fn new(inject_on: Setting, inject: fn(&mut MemoryStore)) -> Self {
            let mut inner = MemoryStore::default();
            inner.values.insert(Setting::WinV.name(), RawValue::string("V"));
            inner.values.insert(Setting::History.name(), RawValue::dword(1));
            inner.values.insert(Setting::Cloud.name(), RawValue::dword(1));
            Self { inner, inject_on, inject: Some(inject), fail_cloud: false, value_writes: Vec::new() }
        }
    }

    impl SettingsStore for DuringBeginStore {
        fn current(&self, setting: Setting) -> io::Result<Option<RawValue>> {
            self.inner.current(setting)
        }
        fn receipt(&self, setting: Setting) -> io::Result<Option<Receipt>> {
            self.inner.receipt(setting)
        }
        fn write_current(&mut self, setting: Setting, value: Option<&RawValue>) -> io::Result<()> {
            if matches!(setting, Setting::Cloud) && self.fail_cloud {
                self.fail_cloud = false;
                return Err(io::Error::other("injected begin cloud failure"));
            }
            self.inner.write_current(setting, value)?;
            self.value_writes.push(setting.name());
            if setting.index() == self.inject_on.index() {
                if let Some(inject) = self.inject.take() { inject(&mut self.inner); }
            }
            Ok(())
        }
        fn write_receipt(&mut self, setting: Setting, receipt: Option<&Receipt>) -> io::Result<()> {
            self.inner.write_receipt(setting, receipt)
        }
    }

    #[test]
    fn cancel_preserves_an_unwritten_hotkey_changed_inside_begin() {
        let mut store = DuringBeginStore::new(Setting::History, |inner| {
            inner.values.insert(Setting::WinV.name(), RawValue::string("XV"));
        });
        let (_, mut change) = stage_change(&mut store, true).unwrap();
        assert!(change.writes[Setting::WinV.index()].is_none());
        assert!(!store.value_writes.contains(&Setting::WinV.name()));
        rollback_change(&mut store, &mut change).unwrap();
        assert_eq!(store.current(Setting::WinV).unwrap(), Some(RawValue::string("XV")));
        assert!(!store.value_writes.contains(&Setting::WinV.name()));
        assert_eq!(store.current(Setting::History).unwrap(), Some(RawValue::dword(1)));
        assert_eq!(store.current(Setting::Cloud).unwrap(), Some(RawValue::dword(1)));
        assert!(store.inner.receipts.is_empty());
    }

    #[test]
    fn cancel_does_not_claim_a_receipt_written_externally_inside_begin() {
        let mut store = DuringBeginStore::new(Setting::History, |inner| {
            inner.values.insert(Setting::WinV.name(), RawValue::string("XV"));
            inner.receipts.insert(Setting::WinV.name(), Receipt { version: 1,
                original: Some(RawValue::string("V")), expected: RawValue::string("XV") });
        });
        let (_, mut change) = stage_change(&mut store, true).unwrap();
        let external = read_state(&store, Setting::WinV).unwrap();
        assert!(change.writes[Setting::WinV.index()].is_none());
        rollback_change(&mut store, &mut change).unwrap();
        assert!(read_state(&store, Setting::WinV).unwrap() == external);
    }

    #[test]
    fn journal_expects_its_own_write_not_an_external_value_observed_at_end_of_begin() {
        let mut store = DuringBeginStore::new(Setting::Cloud, |inner| {
            inner.values.insert(Setting::History.name(), RawValue::dword(9));
        });
        let (_, mut change) = stage_change(&mut store, true).unwrap();
        assert_eq!(change.writes[Setting::History.index()].as_ref().unwrap().expected.value,
            Some(RawValue::dword(0)));
        rollback_change(&mut store, &mut change).unwrap();
        assert_eq!(store.current(Setting::History).unwrap(), Some(RawValue::dword(9)));
        assert_eq!(store.current(Setting::Cloud).unwrap(), Some(RawValue::dword(1)));
        assert_eq!(store.value_writes.iter().filter(|name| **name == Setting::History.name()).count(), 1);
    }

    #[test]
    fn partial_begin_failure_does_not_roll_back_an_external_hotkey_write() {
        let mut store = DuringBeginStore::new(Setting::History, |inner| {
            inner.values.insert(Setting::WinV.name(), RawValue::string("XV"));
        });
        store.fail_cloud = true;
        assert!(stage_change(&mut store, true).is_err());
        assert_eq!(store.current(Setting::WinV).unwrap(), Some(RawValue::string("XV")));
        assert!(!store.value_writes.contains(&Setting::WinV.name()));
        assert_eq!(store.current(Setting::History).unwrap(), Some(RawValue::dword(1)));
        assert_eq!(store.current(Setting::Cloud).unwrap(), Some(RawValue::dword(1)));
        assert!(store.inner.receipts.is_empty());
    }

    #[test]
    fn a_receipt_only_journal_never_rewrites_an_unmodified_target_value() {
        let mut store = DuringBeginStore::new(Setting::Cloud, |_| {});
        let mut change = {
            let mut recording = RecordingStore { store: &mut store, change: SettingsChange::default() };
            recording.write_receipt(Setting::History, Some(&Receipt { version: 1,
                original: Some(RawValue::dword(1)), expected: RawValue::dword(0) })).unwrap();
            recording.change
        };
        assert!(!change.writes[Setting::History.index()].as_ref().unwrap().value_written);
        assert!(!rollback_change(&mut store, &mut change).unwrap());
        assert!(store.value_writes.is_empty());
        assert!(store.inner.receipts.is_empty());
    }

    #[test]
    fn partial_enable_failure_rolls_back_only_the_partial_request() {
        struct FailingStore {
            inner: MemoryStore,
            fail_cloud_once: bool,
        }
        impl SettingsStore for FailingStore {
            fn current(&self, setting: Setting) -> io::Result<Option<RawValue>> {
                self.inner.current(setting)
            }
            fn receipt(&self, setting: Setting) -> io::Result<Option<Receipt>> {
                self.inner.receipt(setting)
            }
            fn write_receipt(
                &mut self,
                setting: Setting,
                receipt: Option<&Receipt>,
            ) -> io::Result<()> {
                self.inner.write_receipt(setting, receipt)
            }
            fn write_current(
                &mut self,
                setting: Setting,
                value: Option<&RawValue>,
            ) -> io::Result<()> {
                if matches!(setting, Setting::Cloud) && self.fail_cloud_once {
                    self.fail_cloud_once = false;
                    return Err(io::Error::other("injected cloud override failure"));
                }
                self.inner.write_current(setting, value)
            }
        }
        let mut store = FailingStore {
            inner: MemoryStore::default(),
            fail_cloud_once: true,
        };
        store
            .inner
            .values
            .insert(Setting::History.name(), RawValue::dword(1));
        store
            .inner
            .values
            .insert(Setting::Cloud.name(), RawValue::dword(1));
        let before = snapshot(&store).unwrap();
        assert!(stage_change(&mut store, true).is_err());
        assert!(snapshot(&store).unwrap() == before);
    }

    #[derive(Default)]
    struct InterruptedRollbackStore {
        inner: MemoryStore,
        fail_value: bool,
        fail_receipt: bool,
    }

    impl SettingsStore for InterruptedRollbackStore {
        fn current(&self, setting: Setting) -> io::Result<Option<RawValue>> {
            self.inner.current(setting)
        }
        fn receipt(&self, setting: Setting) -> io::Result<Option<Receipt>> {
            self.inner.receipt(setting)
        }
        fn write_current(&mut self, setting: Setting, value: Option<&RawValue>) -> io::Result<()> {
            if matches!(setting, Setting::History) && self.fail_value {
                self.fail_value = false;
                return Err(io::Error::other("injected rollback value failure"));
            }
            self.inner.write_current(setting, value)
        }
        fn write_receipt(&mut self, setting: Setting, receipt: Option<&Receipt>) -> io::Result<()> {
            if matches!(setting, Setting::History) && self.fail_receipt {
                self.fail_receipt = false;
                return Err(io::Error::other("injected rollback receipt failure"));
            }
            self.inner.write_receipt(setting, receipt)
        }
    }

    #[test]
    fn interrupted_rollback_retries_both_owned_intermediate_states() {
        for restoring_previous_override in [false, true] {
            let mut store = InterruptedRollbackStore::default();
            store.inner.values.insert(Setting::History.name(), RawValue::dword(1));
            if restoring_previous_override {
                optimize_win_v(&mut store).unwrap();
            }
            let before = snapshot(&store).unwrap();
            let (_, mut change) = stage_change(&mut store, !restoring_previous_override).unwrap();
            store.fail_value = restoring_previous_override;
            store.fail_receipt = !restoring_previous_override;
            assert!(rollback_change(&mut store, &mut change).is_err());
            assert!(snapshot(&store).unwrap() != before);
            rollback_change(&mut store, &mut change).unwrap();
            assert!(snapshot(&store).unwrap() == before);
            assert!(!rollback_change(&mut store, &mut change).unwrap());
        }
    }

    #[test]
    fn retry_after_interruption_still_preserves_an_external_value() {
        let mut store = InterruptedRollbackStore::default();
        optimize_win_v(&mut store).unwrap();
        let (_, mut change) = stage_change(&mut store, false).unwrap();
        store.fail_value = true;
        assert!(rollback_change(&mut store, &mut change).is_err());
        store.inner.values.insert(Setting::History.name(), RawValue::dword(9));
        rollback_change(&mut store, &mut change).unwrap();
        assert_eq!(store.current(Setting::History).unwrap(), Some(RawValue::dword(9)));
    }

    #[test]
    fn lock_name_is_global_and_scoped_to_the_current_user_sid() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"));
        assert_eq!(
            settings_lock_name(&sid),
            format!("Global\\DezirClip.SystemSettingsOwnership.v1.{sid}")
        );
        assert_ne!(
            settings_lock_name("S-1-5-21-101"),
            settings_lock_name("S-1-5-21-102")
        );
    }

    #[test]
    fn mutex_subprocess_probe() {
        let Some(directory) = std::env::var_os("DZC_SETTINGS_LOCK_TEST_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::write(directory.join("waiting"), "waiting").unwrap();
        let _guard = SettingsLock::acquire().unwrap();
        std::fs::write(directory.join("acquired"), "acquired").unwrap();
    }

    #[test]
    fn same_user_processes_wait_for_the_shared_mutex() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let directory =
            std::env::temp_dir().join(format!("dzc-settings-lock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let guard = SettingsLock::acquire().unwrap();
        let test_path = module_path!()
            .split("::")
            .skip(1)
            .collect::<Vec<_>>()
            .join("::");
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("{test_path}::mutex_subprocess_probe"),
                "--nocapture",
            ])
            .env("DZC_SETTINGS_LOCK_TEST_DIR", &directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !directory.join("waiting").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(directory.join("waiting").exists());
        std::thread::sleep(Duration::from_millis(100));
        assert!(!directory.join("acquired").exists());
        drop(guard);
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(directory.join("acquired").exists());
        std::fs::remove_file(directory.join("waiting")).unwrap();
        std::fs::remove_file(directory.join("acquired")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    struct Sandbox {
        root: RegKey,
        prefix: String,
    }

    impl Sandbox {
        fn new() -> Self {
            let prefix = format!(r"Software\DezirClipInstallerTests\{}", uuid::Uuid::new_v4());
            Self {
                root: RegKey::predef(HKEY_CURRENT_USER),
                prefix,
            }
        }
        fn run<T>(
            &self,
            operation: impl FnOnce(&mut RegistryStore<'_>) -> io::Result<T>,
        ) -> io::Result<T> {
            operation(&mut RegistryStore {
                root: &self.root,
                prefix: &self.prefix,
            })
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = self.root.delete_subkey_all(&self.prefix);
        }
    }

    #[test]
    fn registry_restores_types_absence_and_ignores_unowned_policy() {
        let sandbox = Sandbox::new();
        let original = RawValue {
            value_type: REG_EXPAND_SZ as u32,
            bytes: RawValue::string("%USERPROFILE%").bytes,
        };
        sandbox
            .run(|store| {
                store.write_current(Setting::History, Some(&original))?;
                let policy = RawValue::dword(0).to_registry()?;
                store.write(
                    r"Software\Policies\Microsoft\Windows\System",
                    "AllowClipboardHistory",
                    Some(policy),
                )
            })
            .unwrap();
        sandbox.run(|store| optimize_win_v(store)).unwrap();
        sandbox.run(|store| restore_all(store)).unwrap();
        sandbox
            .run(|store| {
                assert_eq!(store.current(Setting::History)?, Some(original));
                assert_eq!(store.current(Setting::Cloud)?, None);
                assert_eq!(store.current(Setting::WinV)?, None);
                assert!(store.receipt(Setting::History)?.is_none());
                assert_eq!(
                    store.read(
                        r"Software\Policies\Microsoft\Windows\System",
                        "AllowClipboardHistory"
                    )?,
                    Some(RawValue::dword(0).to_registry()?)
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn interrupted_changes_keep_receipts_and_can_be_restored_on_retry() {
        let sandbox = Sandbox::new();
        let result: io::Result<()> = sandbox.run(|store| {
            optimize_win_v(store)?;
            Err(io::Error::other("injected operation failure"))
        });
        assert!(result.is_err());
        sandbox
            .run(|store| {
                assert!(store.receipt(Setting::History)?.is_some());
                assert_eq!(store.current(Setting::History)?, Some(RawValue::dword(0)));
                restore_all(store)?;
                for setting in [Setting::History, Setting::Cloud, Setting::WinV] {
                    assert_eq!(store.current(setting)?, None);
                    assert!(store.receipt(setting)?.is_none());
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_receipt_written_before_an_interrupted_target_write_is_safe_to_release() {
        let sandbox = Sandbox::new();
        sandbox
            .run(|store| {
                store.write_receipt(
                    Setting::History,
                    Some(&Receipt {
                        version: 1,
                        original: None,
                        expected: RawValue::dword(0),
                    }),
                )?;
                assert_eq!(store.current(Setting::History)?, None);
                assert!(!restore_all(store)?);
                assert!(store.receipt(Setting::History)?.is_none());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn malformed_receipts_refuse_changes_and_preserve_the_setting() {
        let sandbox = Sandbox::new();
        sandbox
            .run(|store| {
                store.write_current(Setting::History, Some(&RawValue::dword(1)))?;
                store.write(
                    OWNERSHIP_KEY,
                    Setting::History.name(),
                    Some(RegValue {
                        vtype: REG_BINARY,
                        bytes: b"invalid".to_vec(),
                    }),
                )
            })
            .unwrap();
        assert!(sandbox.run(|store| disable_history(store)).is_err());
        assert!(sandbox.run(|store| restore_all(store)).is_err());
        sandbox
            .run(|store| {
                assert_eq!(store.current(Setting::History)?, Some(RawValue::dword(1)));
                Ok(())
            })
            .unwrap();
    }
}
