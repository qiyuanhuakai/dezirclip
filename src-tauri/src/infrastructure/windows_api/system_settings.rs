//! Changes to Windows settings carry an exact-value, write-ahead restoration receipt.
use serde::{Deserialize, Serialize};
use std::io;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
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

#[derive(Clone, Debug, Serialize, Deserialize)]
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

struct SettingsLock(HANDLE);

impl SettingsLock {
    fn acquire() -> io::Result<Self> {
        let name: Vec<u16> = "Local\\DezirClip.SystemSettingsOwnership.v1"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // The handle remains owned by this guard and is released on every exit.
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
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
