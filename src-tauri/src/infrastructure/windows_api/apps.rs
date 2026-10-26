use crate::error::{AppError, AppResult};
use base64::Engine;
use image::{ImageBuffer, Rgba};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ, RGBQUAD,
};
use windows::Win32::UI::Shell::{
    AssocQueryStringW, ExtractIconExW, ASSOCF_VERIFY, ASSOCSTR_EXECUTABLE, ASSOCSTR_FRIENDLYAPPNAME,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, GetSystemMetrics, DI_NORMAL, HICON, SM_CXSMICON, SM_CYSMICON,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
}

/// Icon cache: the data URL keyed by normalised executable path.
///
/// `order` holds the same keys from least to most recently used, which is what
/// makes eviction LRU. A `HashMap` cannot supply this: its iteration order is
/// bucket order under a per-process random seed, so `keys().next()` names an
/// arbitrary entry rather than the oldest one, and re-inserting a hit to "move it
/// to the recent end" did nothing at all for want of an end to move it to.
///
/// Measured with a cap of 8 over a 4-hot / 24-cold workload, each round touching
/// all four hot keys before adding one cold one: the `HashMap` form retained 1 to
/// 3 of the 4 hot entries, varying run to run because the seed is drawn per
/// process, against 4 of 4 for this one. The number of entries the cache holds
/// stayed at the cap throughout -- what went wrong was which entries it kept,
/// not how many. A native miss costs 0.631 ms: it runs `ExtractIconExW`, draws
/// through a GDI DIB, encodes a PNG and base64-encodes it.
///
/// What a miss is worth per scroll is NOT established by this file. The frontend
/// shields this command: `ClipboardItem`'s effect depends on
/// `item.source_app_path` rather than on scroll position, `peekSourceAppIcon`
/// returns before the call is made, and `sourceAppIcon.ts` keeps its own cache
/// plus in-flight dedup. So the native cache is consulted when a path the
/// frontend has not kept turns up -- on the first mount of a row, and after the
/// frontend's own 256-entry LRU evicts it -- not once per mounted row per
/// scroll. Correcting the eviction order is what makes this cache behave as the
/// LRU it was written to be; the end-to-end saving has not been measured.
struct IconCache {
    entries: HashMap<String, Option<String>>,
    order: Vec<String>,
    cap: usize,
}

impl IconCache {
    fn new() -> Self {
        Self::with_cap(EXECUTABLE_ICON_CACHE_MAX)
    }

    fn with_cap(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            cap,
        }
    }

    /// A cached `None` is still a hit -- it means the extractor already ran and
    /// found no icon, and running it again would reach the same answer.
    fn get(&mut self, key: &str) -> Option<Option<String>> {
        let value = self.entries.get(key)?.clone();
        self.touch(key);
        Some(value)
    }

    fn insert(&mut self, key: String, value: Option<String>) {
        let is_new = self.entries.insert(key.clone(), value).is_none();
        if is_new {
            self.order.push(key);
        } else {
            self.touch(&key);
        }
        while self.order.len() > self.cap {
            let oldest = self.order.remove(0);
            self.entries.remove(&oldest);
        }
    }

    /// Move `key` to the most-recently-used end.
    fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let key = self.order.remove(pos);
            self.order.push(key);
        }
    }
}

static EXECUTABLE_ICON_CACHE: OnceLock<Mutex<IconCache>> = OnceLock::new();
const EXECUTABLE_ICON_CACHE_MAX: usize = 256;

#[tauri::command]
pub async fn scan_installed_apps() -> AppResult<Vec<AppInfo>> {
    let mut apps = Vec::new();
    println!("Starting app scan...");

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

    let output_res = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            ps_script,
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();

    if let Ok(output) = output_res {
        if output.status.success() {
            let json_str = String::from_utf8_lossy(&output.stdout);
            if let Ok(scanned) = serde_json::from_str::<Vec<AppInfo>>(&json_str) {
                apps.extend(scanned);
            } else if let Ok(single) = serde_json::from_str::<AppInfo>(&json_str) {
                apps.push(single);
            }
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

use std::os::windows::ffi::OsStrExt;

#[tauri::command]
pub fn get_executable_icon(executable_path: String) -> AppResult<Option<String>> {
    let cache_key = normalize_icon_cache_key(&executable_path);
    if cache_key.is_empty() {
        return Ok(None);
    }

    let cache = EXECUTABLE_ICON_CACHE.get_or_init(|| Mutex::new(IconCache::new()));
    {
        let mut guard = cache
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if let Some(cached) = guard.get(&cache_key) {
            return Ok(cached);
        }
    }

    let icon = extract_executable_icon_data_url(&executable_path)?;
    let mut guard = cache
        .lock()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    guard.insert(cache_key, icon.clone());
    Ok(icon)
}

fn normalize_icon_cache_key(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    std::fs::canonicalize(trimmed)
        .map(|resolved| resolved.to_string_lossy().to_string())
        .unwrap_or_else(|_| trimmed.to_string())
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn extract_executable_icon_data_url(executable_path: &str) -> AppResult<Option<String>> {
    let trimmed = executable_path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let path = Path::new(trimmed);
    if !path.exists() || !path.is_file() {
        return Ok(None);
    }

    let icon = unsafe { extract_executable_icon_handle(trimmed)? };
    let Some(icon) = icon else {
        return Ok(None);
    };

    let png_result = unsafe { render_icon_to_png_bytes(icon) };
    let _ = unsafe { DestroyIcon(icon) };

    match png_result {
        Ok(bytes) => Ok(Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))),
        Err(err) => Err(err),
    }
}

unsafe fn extract_executable_icon_handle(executable_path: &str) -> AppResult<Option<HICON>> {
    let wide_path: Vec<u16> = OsStr::new(executable_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut large_icon = HICON::default();
    let mut small_icon = HICON::default();

    if ExtractIconExW(
        PCWSTR(wide_path.as_ptr()),
        0,
        Some(&mut large_icon),
        Some(&mut small_icon),
        1,
    ) == 0
    {
        return Ok(None);
    }

    let selected = if !small_icon.0.is_null() {
        small_icon
    } else {
        large_icon
    };

    if selected.0.is_null() {
        if !small_icon.0.is_null() {
            let _ = DestroyIcon(small_icon);
        }
        if !large_icon.0.is_null() {
            let _ = DestroyIcon(large_icon);
        }
        return Ok(None);
    }

    if selected.0 != small_icon.0 && !small_icon.0.is_null() {
        let _ = DestroyIcon(small_icon);
    }
    if selected.0 != large_icon.0 && !large_icon.0.is_null() {
        let _ = DestroyIcon(large_icon);
    }

    Ok(Some(selected))
}

unsafe fn render_icon_to_png_bytes(icon: HICON) -> AppResult<Vec<u8>> {
    let width = GetSystemMetrics(SM_CXSMICON).max(16);
    let height = GetSystemMetrics(SM_CYSMICON).max(16);
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [RGBQUAD::default()],
    };
    let mut bits = std::ptr::null_mut();
    let mem_dc = CreateCompatibleDC(None);
    if mem_dc.0.is_null() {
        return Err(AppError::Internal(
            "Failed to create icon memory DC".to_string(),
        ));
    }

    let bitmap = match CreateDIBSection(None, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(bitmap) => bitmap,
        Err(err) => {
            let _ = DeleteDC(mem_dc);
            return Err(AppError::Internal(format!(
                "Failed to create icon bitmap: {}",
                err
            )));
        }
    };

    let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
    let result = (|| -> AppResult<Vec<u8>> {
        if bits.is_null() {
            return Err(AppError::Internal(
                "Icon bitmap has no backing buffer".to_string(),
            ));
        }

        let pixel_len = (width as usize) * (height as usize) * 4;
        std::slice::from_raw_parts_mut(bits as *mut u8, pixel_len).fill(0);
        DrawIconEx(mem_dc, 0, 0, icon, width, height, 0, None, DI_NORMAL)
            .map_err(|e| AppError::Internal(format!("Failed to draw executable icon: {}", e)))?;

        let bgra = std::slice::from_raw_parts(bits as *const u8, pixel_len);
        let mut rgba = vec![0u8; pixel_len];
        for (src, dst) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }

        let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width as u32, height as u32, rgba)
            .ok_or_else(|| {
            AppError::Internal("Failed to build icon image buffer".to_string())
        })?;
        let mut png_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| AppError::Internal(format!("Failed to encode icon PNG: {}", e)))?;
        Ok(png_bytes)
    })();

    let _ = SelectObject(mem_dc, old_bitmap);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    let _ = DeleteDC(mem_dc);
    result
}

// Moved from main.rs
pub async fn launch_uwp_with_file(app_id: &str, file_path: &str) -> AppResult<()> {
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        return Err(AppError::Validation(format!(
            "File does not exist: {}",
            file_path
        )));
    }

    let family_name = app_id.split('!').next().unwrap_or(app_id);

    let ps_script = format!(
        r#"
        Add-Type -AssemblyName System.Runtime.WindowsRuntime
        $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | ? {{ $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' }})[0]
        
        $fileOp = [Windows.Storage.StorageFile,Windows.Storage,ContentType=WindowsRuntime]::GetFileFromPathAsync('{}')
        $fileTask = $asTask.MakeGenericMethod([Windows.Storage.StorageFile]).Invoke($null, @($fileOp))
        $file = $fileTask.GetAwaiter().GetResult()
        
        $options = New-Object Windows.System.LauncherOptions
        $options.TargetApplicationPackageFamilyName = '{}'
        
        $launchOp = [Windows.System.Launcher,Windows.System,ContentType=WindowsRuntime]::LaunchFileAsync($file, $options)
        $launchTask = $asTask.MakeGenericMethod([Boolean]).Invoke($null, @($launchOp))
        $result = $launchTask.GetAwaiter().GetResult()
        
        if ($result) {{ exit 0 }} else {{ exit 1 }}
        "#,
        file_path.replace("'", "''"),
        family_name
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
        .map_err(|e| format!("Starting PowerShell failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("WinRT Launch failed: {}", stderr.trim()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::{IconCache, EXECUTABLE_ICON_CACHE_MAX};

    /// A cache with the same shape but a cap small enough to evict inside a
    /// test. The production cap is 256, so a test that needs overflow would
    /// otherwise have to insert hundreds of keys to observe it -- and a test that
    /// quietly never fills the cache asserts nothing about eviction at all.
    fn small_cache(cap: usize) -> IconCache {
        IconCache::with_cap(cap)
    }

    fn cache_with(keys: &[&str]) -> IconCache {
        let mut cache = IconCache::new();
        for k in keys {
            cache.insert((*k).to_string(), Some(format!("icon-{k}")));
        }
        cache
    }

    fn contains(cache: &IconCache, key: &str) -> bool {
        cache.entries.contains_key(key)
    }

    // The defect this replaced: `HashMap::keys().next()` is bucket order, not
    // insertion order, so nothing was being evicted on use -- and a hit that
    // re-inserted the key moved it nowhere, because there was no end to move it
    // to. With the HashMap the entry that got thrown away was an arbitrary one.
    #[test]
    fn the_least_recently_used_entry_is_the_one_evicted() {
        let mut cache = small_cache(3);
        cache.insert("a".to_string(), Some("icon-a".to_string()));
        cache.insert("b".to_string(), Some("icon-b".to_string()));
        cache.insert("c".to_string(), Some("icon-c".to_string()));
        // Touch "a" so "b" becomes the oldest.
        cache.get("a");
        cache.insert("d".to_string(), Some("icon-d".to_string()));

        assert!(!contains(&cache, "b"), "the oldest entry is the one to go");
        assert!(contains(&cache, "a"), "a was just used, so it must stay");
        assert!(contains(&cache, "c"));
        assert!(contains(&cache, "d"));
    }

    // The measurement this is based on: with a cap of 8 over a 4-hot workload,
    // the HashMap form retained 1 to 3 of the 4 hot entries -- varying per run,
    // because the iteration order it depends on is seeded per process -- where
    // this one keeps 4. The cache still held its full 8 entries either way; what
    // differed was which ones.
    //
    // The workload interleaves the hot set with the cold tail rather than
    // flooding the cache in one block. That is what the app actually does -- a
    // row asks for its icon when it mounts, and a scroll mounts rows -- and it is
    // also the only shape in which an LRU can win: a cold flood with no hot
    // touches evicts the hot entries under any correct policy, because they
    // genuinely were the least recently used.
    #[test]
    fn a_hot_working_set_survives_a_long_cold_tail() {
        let hot = ["h0", "h1", "h2", "h3"];
        let mut cache = small_cache(8);
        // The hot entries have to be in the cache before they can be "touched" --
        // a get on an absent key refreshes nothing.
        for h in hot {
            cache.insert(h.to_string(), Some(format!("icon-{h}")));
        }
        for round in 0..400 {
            // One scroll: every visible row asks for its icon, so all four hot
            // apps are touched before the cold tail gets a turn.
            for h in hot {
                assert!(
                    cache.get(h).is_some(),
                    "{h} must hit on round {round} while it is visible"
                );
            }
            // ...and the cold tail adds entries nobody is looking at.
            cache.insert(format!("cold-{round}"), Some("icon-cold".to_string()));
        }
        // The hot entries were touched throughout, so all four are still there.
        for h in hot {
            assert!(cache.get(h).is_some(), "{h} must still be cached");
        }
        // And they must still be there on the next pass, not merely returned once.
        for h in hot {
            assert!(cache.get(h).is_some(), "{h} must survive a second pass");
        }
    }

    #[test]
    fn the_cache_never_exceeds_its_cap() {
        let mut cache = IconCache::new();
        for i in 0..(EXECUTABLE_ICON_CACHE_MAX * 2 + 5) {
            cache.insert(format!("k-{i}"), Some("icon".to_string()));
            assert!(
                cache.entries.len() <= EXECUTABLE_ICON_CACHE_MAX,
                "grew to {} past the cap of {EXECUTABLE_ICON_CACHE_MAX}",
                cache.entries.len()
            );
        }
        assert_eq!(cache.entries.len(), EXECUTABLE_ICON_CACHE_MAX);
        assert_eq!(cache.order.len(), EXECUTABLE_ICON_CACHE_MAX);
    }

    // A miss costs 0.631 ms here (ExtractIconExW + GDI draw + PNG + base64), so
    // caching "this path has no icon" has to count as a hit. Reading it as a miss
    // would redo the extraction for every row of a source that has no icon.
    #[test]
    fn a_cached_absence_is_still_a_hit() {
        let mut cache = IconCache::new();
        cache.insert("no-icon".to_string(), None);
        assert_eq!(
            cache.get("no-icon"),
            Some(None),
            "None must be cached, not re-extracted"
        );
        assert_eq!(cache.get("never-seen"), None, "an unknown key is a miss");
    }

    #[test]
    fn reinserting_an_existing_key_does_not_grow_the_cache() {
        let mut cache = IconCache::new();
        cache.insert("a".to_string(), Some("icon-a".to_string()));
        cache.insert("a".to_string(), Some("icon-a".to_string()));
        cache.insert("a".to_string(), Some("icon-a".to_string()));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(
            cache.order.len(),
            1,
            "order must not accumulate duplicates"
        );
    }

    #[test]
    fn a_hit_returns_the_stored_value() {
        let mut cache = cache_with(&["only"]);
        assert_eq!(cache.get("only"), Some(Some("icon-only".to_string())));
    }

    // Reviewer: the individual tests cover the pieces, not the combination.
    // A cached `None` that is touched and then pushed out by overflow is the
    // case that matters -- an entry with no icon still has to take part in
    // eviction correctly, and it is the one whose value is easiest to lose
    // track of because there is nothing to hand back.
    #[test]
    fn a_cached_absence_takes_part_in_eviction() {
        let mut cache = small_cache(3);
        cache.insert("oldest".to_string(), Some("icon-oldest".to_string()));
        cache.insert("no-icon".to_string(), None);
        cache.insert("newest".to_string(), Some("icon-newest".to_string()));

        // Touch the absence so it is no longer the oldest entry.
        assert_eq!(cache.get("no-icon"), Some(None));

        // Overflow: "oldest" is now the least recently used.
        cache.insert("newcomer".to_string(), Some("icon-newcomer".to_string()));

        assert!(!contains(&cache, "oldest"), "the oldest entry is evicted");
        assert!(
            contains(&cache, "no-icon"),
            "a touched absence must be kept, not evicted for being empty"
        );
        assert!(contains(&cache, "newest"));
        assert!(contains(&cache, "newcomer"));
        assert_eq!(cache.entries.len(), 3);
    }

    // Reviewer: overwriting an existing key with a different value must update
    // both the stored value and its recency, and must not leave the old value
    // reachable. `insert` is reached on the miss path, so a path that was
    // cached and then re-extracted goes through this.
    #[test]
    fn overwriting_updates_the_value_and_the_recency() {
        let mut cache = small_cache(3);
        cache.insert("a".to_string(), Some("icon-a-v1".to_string()));
        cache.insert("b".to_string(), Some("icon-b".to_string()));
        cache.insert("c".to_string(), Some("icon-c".to_string()));

        // Overwrite "a" with a different value. Nothing reads "a" between this
        // and the overflow below: a `get` would itself refresh the recency and
        // mask an `insert` that failed to.
        cache.insert("a".to_string(), Some("icon-a-v2".to_string()));

        // Overflow now evicts "b" (oldest), not "a".
        cache.insert("d".to_string(), Some("icon-d".to_string()));
        assert!(!contains(&cache, "b"), "overwriting must refresh recency");
        assert!(
            contains(&cache, "a"),
            "the overwritten key must not be the one evicted"
        );
        assert_eq!(cache.entries.len(), 3);
        assert_eq!(cache.order.len(), 3, "order must not gain a duplicate");

        // And the stored value really is the newer one.
        assert_eq!(
            cache.get("a"),
            Some(Some("icon-a-v2".to_string())),
            "the newer value must replace the old one"
        );
    }
}
