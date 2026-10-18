//! Embedded editing-buffer widgets use the same native dzc executable.

use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Nu,
    Pwsh,
}

pub fn run(shell: Shell, install: Option<&Path>) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable = executable
        .to_str()
        .ok_or_else(|| "Shell widgets require a UTF-8 executable path".to_string())?;
    let script = render(shell, executable)?;
    if let Some(path) = install {
        if shell != Shell::Nu {
            return Err("--install is only available for Nushell".to_string());
        }
        install_nu(path, &script)?;
        println!("Nushell widget ready: {}", path.display());
        println!("Restart Nushell if this file is in a user autoload directory.");
        return Ok(());
    }
    std::io::stdout()
        .lock()
        .write_all(script.as_bytes())
        .map_err(|error| error.to_string())
}

fn render(shell: Shell, executable: &str) -> Result<String, String> {
    let (template, quoted) = match shell {
        Shell::Bash => (include_str!("shell/bash.sh"), posix_quote(executable)),
        Shell::Zsh => (include_str!("shell/zsh.zsh"), posix_quote(executable)),
        Shell::Fish => (
            include_str!("shell/fish.fish"),
            format!(
                "'{}'",
                executable.replace('\\', "\\\\").replace('\'', "\\'")
            ),
        ),
        Shell::Nu => (
            include_str!("shell/nu.nu"),
            serde_json::to_string(executable).map_err(|error| error.to_string())?,
        ),
        Shell::Pwsh => (
            include_str!("shell/pwsh.ps1"),
            format!("'{}'", executable.replace('\'', "''")),
        ),
    };
    Ok(template.replace("__DZC_EXECUTABLE__", &quoted))
}

fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn install_nu(path: &Path, script: &str) -> Result<(), String> {
    if path.extension().is_none_or(|extension| extension != "nu") {
        return Err("Choose a .nu file in $nu.user-autoload-dirs.0".to_string());
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_file()
                && std::fs::read(path).map_err(|error| error.to_string())? == script.as_bytes()
            {
                return Ok(());
            }
            return Err(format!(
                "Refusing to overwrite existing Nushell file: {}. Choose a new path or review and remove it manually.",
                path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(script.as_bytes())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_widgets_use_native_dzc_and_quote_paths_without_execution() {
        let executable = "/tmp/中文 path/'literal'/$()\\dzc";
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Nu, Shell::Pwsh] {
            let script = render(shell, executable).unwrap();
            assert!(!script.contains("__DZC_EXECUTABLE__"));
            assert!(script.contains("DZC_BIN"));
            assert!(script.contains("pick"));
            assert!(script.contains("insert"));
            assert!(!script.contains("python"));
        }
        assert_eq!(posix_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn nushell_install_is_explicit_idempotent_and_preserves_conflicts() {
        let directory = std::env::temp_dir().join(format!(
            "dzc-shell-install-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = directory.join("autoload/dzc.nu");
        install_nu(&path, "content\n").unwrap();
        install_nu(&path, "content\n").unwrap();
        assert!(install_nu(&path, "replacement\n").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "content\n");
        assert!(install_nu(&directory.join("wrong.txt"), "content").is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
