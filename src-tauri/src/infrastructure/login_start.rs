//! Both desktops use the same per-user login entry. Only its current owner may disable it.
use std::{fs, io, path::Path};

pub fn enabled(executable: &Path) -> io::Result<bool> {
    platform::enabled(executable)
}

pub fn set(executable: &Path, enabled: bool) -> io::Result<()> {
    let config = dirs::config_local_dir()
        .ok_or_else(|| io::Error::other("configuration directory unavailable"))?;
    let directory = config.join("dezirclip");
    fs::create_dir_all(&directory)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("login-start.lock"))?;
    lock.lock()?;
    platform::set(executable, enabled)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::{io::Write, path::PathBuf};

    fn entry_path() -> io::Result<PathBuf> {
        Ok(dirs::config_dir()
            .ok_or_else(|| io::Error::other("configuration directory unavailable"))?
            .join("autostart/dezirclip.desktop"))
    }

    fn command(executable: &Path) -> io::Result<String> {
        let path = executable
            .to_str()
            .filter(|s| !s.contains('=') && !s.chars().any(char::is_control))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsupported desktop entry path",
                )
            })?;
        // The desktop-entry string is unescaped once before Exec argument quoting is interpreted.
        let escaped = path
            .replace('\\', "\\\\\\\\")
            .replace('"', "\\\\\"")
            .replace('$', "\\\\$")
            .replace('`', "\\\\`")
            .replace('%', "%%");
        // GIO probes the first executable before expanding percent escapes.
        let launcher = if path.contains('%') { "/usr/bin/env -- " } else { "" };
        Ok(format!("Exec={launcher}\"{escaped}\" --minimized"))
    }

    fn registration(entry: &str, executable: &Path) -> io::Result<Option<bool>> {
        let expected = command(executable)?;
        let legacy = format!("Exec=\"{}\" --minimized", executable.display());
        let mut desktop = false;
        let mut owned = false;
        let mut disabled = false;
        for line in entry.lines().map(str::trim) {
            if line.starts_with('[') {
                desktop = line == "[Desktop Entry]";
            }
            if desktop && line.starts_with("Exec=") {
                owned = line == expected || line == legacy;
            }
            if desktop && matches!(line, "Hidden=true" | "X-GNOME-Autostart-enabled=false") {
                disabled = true;
            }
        }
        Ok(owned.then_some(!disabled))
    }

    fn read() -> io::Result<String> {
        let path = entry_path()?;
        match fs::metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
            Err(e) => return Err(e),
            Ok(meta) if !meta.is_file() || meta.len() > 64 * 1024 => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid desktop login entry",
                ));
            }
            Ok(_) => {}
        }
        fs::read_to_string(path)
    }

    pub(super) fn enabled(executable: &Path) -> io::Result<bool> {
        Ok(registration(&read()?, executable)? == Some(true))
    }

    pub(super) fn set(executable: &Path, enabled: bool) -> io::Result<()> {
        let path = entry_path()?;
        if enabled {
            let command = command(executable)?;
            let directory = path.parent().unwrap();
            fs::create_dir_all(directory)?;
            let mut file = tempfile::NamedTempFile::new_in(directory)?;
            write!(file, "[Desktop Entry]\nType=Application\nName=DezirClip\n{command}\nHidden=false\nNoDisplay=false\nX-GNOME-Autostart-enabled=true\nStartupNotify=false\nStartupWMClass=DezirClip\n")?;
            file.persist(path)?;
        } else if registration(&read()?, executable)?.is_some() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE},
        RegKey,
    };
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn value(key: &RegKey, name: &str) -> io::Result<Option<String>> {
        match key.get_value(name) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn command(executable: &Path) -> String {
        format!("\"{}\" --minimized", executable.display())
    }

    fn matches(value: &str, executable: &Path) -> bool {
        value.eq_ignore_ascii_case(&command(executable))
    }

    pub(super) fn enabled(executable: &Path) -> io::Result<bool> {
        let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            other => other?,
        };
        enabled_in(&key, executable)
    }

    fn enabled_in(key: &RegKey, executable: &Path) -> io::Result<bool> {
        for name in ["DezirClip", "tie-z"] {
            if value(key, name)?.is_some_and(|v| matches(&v, executable)) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn set(executable: &Path, enabled: bool) -> io::Result<()> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey_with_flags(RUN, KEY_READ | KEY_WRITE)?;
        set_in(&key, executable, enabled)
    }

    fn set_in(key: &RegKey, executable: &Path, enabled: bool) -> io::Result<()> {
        if enabled {
            let cmd = command(executable);
            if cmd.encode_utf16().count() > 260 {
                return Err(io::Error::other("login command exceeds 260 characters"));
            }
            key.set_value("DezirClip", &cmd)?;
            if value(key, "tie-z")?.is_some() {
                key.delete_value("tie-z")?;
            }
        } else {
            for name in ["DezirClip", "tie-z"] {
                if value(key, name)?.is_some_and(|v| matches(&v, executable)) {
                    key.delete_value(name)?;
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    #[test]
    fn switches_between_installations_without_removing_the_other_startup_command() {
        // Exercise normal registry value semantics in a disposable key outside the Windows Run locations.
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let name = format!(r"Software\DezirClip-login-test-{}", uuid::Uuid::new_v4());
        let (key, _) = root.create_subkey(&name).unwrap();
        let tauri = Path::new(r"C:\Program Files\DezirClip\dezirclip.exe");
        let mygo = Path::new(r"C:\MyGo 测试\dzc-desktop.exe");
        key.set_value("tie-z", &command(tauri)).unwrap();
        assert!(enabled_in(&key, tauri).unwrap());
        set_in(&key, mygo, true).unwrap();
        assert_eq!(value(&key, "DezirClip").unwrap(), Some(command(mygo)));
        assert!(value(&key, "tie-z").unwrap().is_none());
        set_in(&key, tauri, false).unwrap();
        assert!(enabled_in(&key, mygo).unwrap());
        set_in(&key, tauri, true).unwrap();
        set_in(&key, mygo, false).unwrap();
        assert!(enabled_in(&key, tauri).unwrap());
        set_in(&key, tauri, false).unwrap();
        assert!(value(&key, "DezirClip").unwrap().is_none());
        drop(key);
        root.delete_subkey(name).unwrap();
    }
}
