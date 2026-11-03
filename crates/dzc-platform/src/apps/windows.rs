use serde::{Deserialize, Serialize};
use std::{
    io,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::Path,
    process::Command,
};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::UI::Shell::{
    AssocQueryStringW, ASSOCF_VERIFY, ASSOCSTR_EXECUTABLE, ASSOCSTR_FRIENDLYAPPNAME,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
}

pub fn scan_installed_apps() -> io::Result<Vec<AppInfo>> {
    let mut apps = Vec::new();

    // 1. Add known system apps directly (Backend Fallback)
    let sys_root = std::env::var("SystemRoot").unwrap_or("C:\\Windows".to_string());

    let common_apps = vec![
        (
            "Notepad (记事本)",
            format!(r"{}\System32\notepad.exe", sys_root),
        ),
        (
            "Paint (画图)",
            format!(r"{}\System32\mspaint.exe", sys_root),
        ),
        (
            "Calculator (计算器)",
            format!(r"{}\System32\calc.exe", sys_root),
        ),
        (
            "Command Prompt (CMD)",
            format!(r"{}\System32\cmd.exe", sys_root),
        ),
        (
            "PowerShell",
            format!(
                r"{}\System32\WindowsPowerShell\v1.0\powershell.exe",
                sys_root
            ),
        ),
        ("Registry Editor", format!(r"{}\regedit.exe", sys_root)),
        (
            "Snipping Tool",
            format!(r"{}\System32\SnippingTool.exe", sys_root),
        ),
        ("Explorer", format!(r"{}\explorer.exe", sys_root)),
    ];

    for (name, path) in common_apps {
        if Path::new(&path).exists() {
            apps.push(AppInfo {
                name: name.to_string(),
                path,
            });
        }
    }

    // Check for common browsers
    let program_files = std::env::var("ProgramFiles").unwrap_or(r"C:\Program Files".to_string());
    let program_files_x86 =
        std::env::var("ProgramFiles(x86)").unwrap_or(r"C:\Program Files (x86)".to_string());

    let chrome_path = format!(r"{}\Google\Chrome\Application\chrome.exe", program_files);
    if Path::new(&chrome_path).exists() {
        apps.push(AppInfo {
            name: "Google Chrome".to_string(),
            path: chrome_path,
        });
    }

    let edge_path = format!(
        r"{}\Microsoft\Edge\Application\msedge.exe",
        program_files_x86
    );
    if Path::new(&edge_path).exists() {
        apps.push(AppInfo {
            name: "Microsoft Edge".to_string(),
            path: edge_path,
        });
    }

    // 2. Run PowerShell Scan (Best Effort)
    let ps_script = r#"
        $ErrorActionPreference = 'SilentlyContinue'
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8

        $apps = Get-StartApps | Select-Object Name, AppID

        $results = New-Object System.Collections.Generic.List[Object]
        foreach ($app in $apps) {
            if (![string]::IsNullOrEmpty($app.AppID)) {
                $obj = @{ name = $app.Name; path = $app.AppID }
                $results.Add($obj)
            }
        }

        if ($results.Count -eq 0) {
            Write-Output "[]"
        } else {
            $results | ConvertTo-Json -Depth 2 -Compress
        }
    "#;

    let mut query =
        Command::new(Path::new(&sys_root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"));
    query
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            ps_script,
        ])
        .creation_flags(0x08000000);

    if let Ok(json) = scan_output(query) {
        if let Ok(scanned) = serde_json::from_slice::<Vec<AppInfo>>(&json) {
            apps.extend(scanned);
        } else if let Ok(single) = serde_json::from_slice::<AppInfo>(&json) {
            apps.push(single);
        }
    }

    // 3. Deduplicate and Filter
    let invalid_keywords = [
        "uninstall",
        "卸载",
        "setup",
        "install",
        "config",
        "help",
        "readme",
        "update",
        "修复",
        "remove",
    ];

    apps.retain(|app| {
        let name_lower = app.name.to_lowercase();
        !invalid_keywords.iter().any(|&k| name_lower.contains(k))
    });

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.path.eq_ignore_ascii_case(&b.path));

    Ok(apps)
}

fn scan_output(mut query: Command) -> io::Result<Vec<u8>> {
    use std::{
        io::{Read, Seek},
        process::Stdio,
        time::{Duration, Instant},
    };
    const LIMIT: u64 = 8 * 1024 * 1024;
    let mut file = tempfile::tempfile()?;
    let mut child = query
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(file.try_clone()?)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let result = (|| {
        while Instant::now() < deadline && file.metadata()?.len() <= LIMIT {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err(io::Error::other("Application scan failed"));
                }
                file.rewind()?;
                let mut bytes = Vec::new();
                file.take(LIMIT + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > LIMIT {
                    return Err(io::Error::other("Application list too large"));
                }
                return Ok(bytes);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Err(io::Error::other("Application scan exceeded its limit"))
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

pub fn get_system_default_app(content_type: String) -> io::Result<String> {
    let ext = match content_type.as_str() {
        "image" => ".png",
        "video" => ".mp4",
        "code" => ".txt",
        "text" | "rich_text" => ".txt",
        "file" => ".txt",
        "link" | "url" => "http",
        _ => return Ok("系统默认".to_string()),
    };

    unsafe {
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ext_wide: Vec<u16> = std::ffi::OsStr::new(ext)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let ext_pcwstr = PCWSTR(ext_wide.as_ptr());

        let res = AssocQueryStringW(
            ASSOCF_VERIFY,
            ASSOCSTR_FRIENDLYAPPNAME,
            ext_pcwstr,
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut size,
        );

        if res.is_ok() {
            let len = (0..size as usize)
                .position(|i| buffer[i] == 0)
                .unwrap_or(size as usize);
            let name = String::from_utf16_lossy(&buffer[0..len]);
            if !name.trim().is_empty() {
                return Ok(name);
            }
        }

        // Fallback to executable name
        let mut size = buffer.len() as u32;
        let res = AssocQueryStringW(
            ASSOCF_VERIFY,
            ASSOCSTR_EXECUTABLE,
            ext_pcwstr,
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut size,
        );
        if res.is_ok() {
            let len = (0..size as usize)
                .position(|i| buffer[i] == 0)
                .unwrap_or(size as usize);
            let path_str = String::from_utf16_lossy(&buffer[0..len]);
            if let Some(name) = Path::new(&path_str).file_name() {
                return Ok(name.to_string_lossy().to_string());
            }
        }
    }

    Ok("系统默认".to_string())
}
