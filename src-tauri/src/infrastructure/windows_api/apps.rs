use crate::error::{AppError, AppResult};
use dzc_platform::apps::{self, AppInfo};
use std::os::windows::process::CommandExt;
use std::process::Command;

#[tauri::command]
pub async fn scan_installed_apps() -> AppResult<Vec<AppInfo>> {
    apps::scan_installed_apps().map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn get_associated_apps(extension: String) -> AppResult<Vec<AppInfo>> {
    let ext = if extension.starts_with('.') {
        extension.clone()
    } else {
        format!(".{}", extension)
    };

    let ps_script = format!(
        r#"
        $ErrorActionPreference = 'SilentlyContinue'
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
        
        $ext = "{}"
        $list = New-Object System.Collections.Generic.List[Object]
        $addedPaths = New-Object System.Collections.Generic.HashSet[String]

        $regPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$ext\OpenWithList"
        if (Test-Path $regPath) {{
            $mru = Get-ItemProperty $regPath
            $mru.PSObject.Properties | Where-Object {{ $_.Name -match "^[a-zA-Z]$" }} | ForEach-Object {{
                $exeName = $_.Value
                if ($exeName -and $exeName.EndsWith(".exe")) {{
                    try {{
                        $cmd = Get-Command $exeName -ErrorAction SilentlyContinue
                        if ($cmd) {{
                            $fullPath = $cmd.Source
                             if (-not $addedPaths.Contains($fullPath)) {{
                                $list.Add(@{{ name = $cmd.Name; path = $fullPath }})
                                $addedPaths.Add($fullPath) | Out-Null
                             }}
                        }} else {{
                            $appPathKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\$exeName"
                            if (Test-Path $appPathKey) {{
                                $fullPath = (Get-ItemProperty $appPathKey).'(default)'
                                if ($fullPath -and (Test-Path $fullPath)) {{
                                    if (-not $addedPaths.Contains($fullPath)) {{
                                        $list.Add(@{{ name = $exeName; path = $fullPath }})
                                        $addedPaths.Add($fullPath) | Out-Null
                                    }}
                                }}
                            }}
                        }}
                    }} catch {{}}
                }}
            }}
        }}

        $list | ConvertTo-Json -Depth 2 -Compress
    "#,
        ext
    );

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_script,
        ])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let json_str = String::from_utf8_lossy(&output.stdout);
        if let Ok(apps) = serde_json::from_str::<Vec<AppInfo>>(&json_str) {
            return Ok(apps);
        }
    }

    Ok(Vec::new())
}

#[tauri::command]
pub fn get_system_default_app(content_type: String) -> AppResult<String> {
    apps::get_system_default_app(content_type).map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub fn get_executable_icon(executable_path: String) -> AppResult<Option<String>> {
    apps::get_executable_icon(executable_path).map_err(|error| AppError::Internal(error.to_string()))
}
