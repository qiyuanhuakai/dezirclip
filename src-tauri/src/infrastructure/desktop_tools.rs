use super::cli_path;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Debug, Clone, Serialize)]
pub struct CliInfo {
    pub cli_path: Option<String>,
    pub cli_version: String,
    pub skill_path: Option<String>,
    pub theme_skill_path: Option<String>,
    pub cli_on_path: bool,
    pub path_cli_path: Option<String>,
    pub path_cli_version: Option<String>,
    pub cli_path_conflict: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CliPathResult {
    pub installed_path: String,
    pub path_entry: String,
    pub already_linked: bool,
    pub requires_new_terminal: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentSkill {
    DzcCli,
    DezirclipTheme,
}

impl AgentSkill {
    pub fn name(self) -> &'static str {
        match self {
            Self::DzcCli => "dzc-cli",
            Self::DezirclipTheme => "dezirclip-theme",
        }
    }
}

pub fn cli_info(bundled: Option<PathBuf>, skill: impl Fn(AgentSkill) -> Option<PathBuf>) -> CliInfo {
    let path_cli_path = find_cli_on_path();
    let cli_on_path = path_cli_path.is_some();
    let cli_path = bundled
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| path_cli_path.clone());
    let cli_path_conflict = match (&cli_path, &path_cli_path) {
        (Some(selected), Some(on_path)) => {
            cli_path::cli_paths_differ(Path::new(selected), Path::new(on_path))
        }
        _ => false,
    };
    let selected_version = cli_path
        .as_deref()
        .and_then(|path| cli_path::read_cli_version(Path::new(path)));
    let path_cli_version = if cli_path_conflict {
        path_cli_path
            .as_deref()
            .and_then(|path| cli_path::read_cli_version(Path::new(path)))
    } else if cli_on_path {
        selected_version.clone()
    } else {
        None
    };
    let cli_version = selected_version.unwrap_or_else(|| "-".to_string());
    let skill_path = skill(AgentSkill::DzcCli)
        .map(|path| path.to_string_lossy().to_string());
    let theme_skill_path = skill(AgentSkill::DezirclipTheme)
        .map(|path| path.to_string_lossy().to_string());

    CliInfo {
        cli_path,
        cli_version,
        skill_path,
        theme_skill_path,
        cli_on_path,
        path_cli_path,
        path_cli_version,
        cli_path_conflict,
    }
}

pub fn install_cli_path(bundled: Option<PathBuf>) -> Result<CliPathResult, String> {
    let cli_path = bundled
        .or_else(|| find_cli_on_path().map(PathBuf::from))
        .ok_or_else(|| "dzc executable not found".to_string())?;
    let result = cli_path::add_cli_to_path(&cli_path)?;
    Ok(CliPathResult {
        installed_path: result.installed_path.to_string_lossy().to_string(),
        path_entry: result.path_entry.to_string_lossy().to_string(),
        already_linked: result.already_linked,
        requires_new_terminal: result.requires_new_terminal,
    })
}

fn find_cli_on_path() -> Option<String> {
    let path_value = std::env::var_os("PATH")?;
    cli_path::find_cli_on_path_in(&path_value, cli_binary_name())
        .map(|path| path.to_string_lossy().to_string())
}

fn cli_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "dzc.exe"
    } else {
        "dzc"
    }
}

pub fn find_filesystem_skill_path_in(dir: &Path, skill: AgentSkill) -> Option<PathBuf> {
    [dir.join("skills"), dir.join("../share/dezirclip/skills"), dir.join("../skills"), dir.join("../../skills")]
        .into_iter()
        .map(|root| root.join(skill.name()))
        .find(|path| path.join("SKILL.md").is_file())
}

pub fn install_folder_from_skill_path(skill_path: &Path) -> Option<PathBuf> {
    skill_path
        .parent()
        .and_then(|skills_dir| skills_dir.parent())
        .map(Path::to_path_buf)
}

pub fn open_folder(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_folder_requests_accept_only_bundled_skill_names() {
        for (json, name) in [("\"dzc-cli\"", "dzc-cli"), ("\"dezirclip-theme\"", "dezirclip-theme")] {
            assert_eq!(serde_json::from_str::<AgentSkill>(json).unwrap().name(), name);
        }
        for json in ["\"../dezirclip-theme\"", "\"C:/secrets\"", "\"unknown\"", "\"\""] {
            assert!(serde_json::from_str::<AgentSkill>(json).is_err());
        }
    }

    #[test]
    fn skills_resolve_independently_and_require_their_manifest() {
        let fixture = std::env::temp_dir().join(format!("dezirclip-agent-skills-{}", uuid::Uuid::new_v4()));
        let executable_dir = fixture.join("bin/debug");
        let cli = executable_dir.join("skills/dzc-cli");
        let empty_theme = executable_dir.join("skills/dezirclip-theme");
        let theme = fixture.join("skills/dezirclip-theme");
        std::fs::create_dir_all(&cli).unwrap();
        std::fs::create_dir_all(&empty_theme).unwrap();
        std::fs::write(cli.join("SKILL.md"), "CLI").unwrap();
        assert_eq!(find_filesystem_skill_path_in(&executable_dir, AgentSkill::DzcCli), Some(cli));
        assert_eq!(find_filesystem_skill_path_in(&executable_dir, AgentSkill::DezirclipTheme), None);
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join("SKILL.md"), "Theme").unwrap();
        assert_eq!(
            find_filesystem_skill_path_in(&executable_dir, AgentSkill::DezirclipTheme).unwrap().canonicalize().unwrap(),
            theme.canonicalize().unwrap()
        );
        let flatpak_executable_dir = fixture.join("app/bin");
        let flatpak_theme = fixture.join("app/share/dezirclip/skills/dezirclip-theme");
        std::fs::create_dir_all(&flatpak_executable_dir).unwrap();
        std::fs::create_dir_all(&flatpak_theme).unwrap();
        std::fs::write(flatpak_theme.join("SKILL.md"), "Bundled theme").unwrap();
        assert_eq!(
            find_filesystem_skill_path_in(&flatpak_executable_dir, AgentSkill::DezirclipTheme).unwrap().canonicalize().unwrap(),
            flatpak_theme.canonicalize().unwrap()
        );
        let resolved = fixture.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("dezirclip-agent-skills-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn test_install_folder_from_skill_path_returns_app_root() {
        let skill_path = PathBuf::from("/opt/dezirclip/skills/dzc-cli");

        assert_eq!(
            install_folder_from_skill_path(&skill_path),
            Some(PathBuf::from("/opt/dezirclip"))
        );
    }
}
