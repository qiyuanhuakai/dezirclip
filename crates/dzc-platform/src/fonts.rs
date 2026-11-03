use dzc_protocol::wire::DesktopFont as FontInfo;
#[cfg(feature = "native-fonts")]
use font_kit::{handle::Handle, source::SystemSource};

trait FontSource {
    fn all_fonts(&self) -> Result<Vec<FontInfo>, String>;
}

struct InstalledFonts;

#[cfg(feature = "native-fonts")]
impl FontSource for InstalledFonts {
    fn all_fonts(&self) -> Result<Vec<FontInfo>, String> {
        let handles = SystemSource::new()
            .all_fonts()
            .map_err(|e| format!("system source enumeration failed: {e}"))?;

        let mut fonts: Vec<FontInfo> = Vec::with_capacity(handles.len());
        for handle in handles {
            match &handle {
                Handle::Path { path, .. } => {
                    let font = match handle.load() {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    let path_str = path.to_string_lossy().into_owned();
                    let family = font.family_name();
                    let is_mono = font.is_monospace();
                    fonts.push(FontInfo {
                        family,
                        is_mono,
                        path: path_str,
                    });
                }
                Handle::Memory { .. } => {
                    continue;
                }
            }
        }

        Ok(sort_fonts(dedup_fonts(fonts)))
    }
}

// The standalone Linux CLI has a glibc 2.28 ABI contract. Query Fontconfig
// only when a desktop requests the picker, without linking a new library.
#[cfg(all(target_os = "linux", not(feature = "native-fonts")))]
impl FontSource for InstalledFonts {
    fn all_fonts(&self) -> Result<Vec<FontInfo>, String> {
        let mut command = std::process::Command::new("fc-list");
        command.args(["--format", "%{family[0]}\x1f%{spacing}\x1f%{file}\x1e"]);
        let output = fontconfig_output(command, std::time::Duration::from_secs(3), 4 * 1024 * 1024)
            .map_err(|error| error.to_string())?;
        let data = String::from_utf8_lossy(&output);
        let fonts = data.split('\x1e').filter_map(|line| {
            let mut fields = line.splitn(3, '\x1f');
            let family = fields.next()?;
            let spacing = fields.next()?;
            let path = fields.next()?;
            if family.is_empty() || path.is_empty() { return None; }
            Some(FontInfo { family: family.into(), is_mono: matches!(spacing, "100" | "110"), path: path.into() })
        }).collect();
        Ok(sort_fonts(dedup_fonts(fonts)))
    }
}

#[cfg(all(target_os = "linux", not(feature = "native-fonts")))]
fn fontconfig_output(mut command: std::process::Command, timeout: std::time::Duration, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::{io::{self, Read}, os::{fd::OwnedFd, unix::net::UnixStream}, process::Stdio, time::{Duration, Instant}};
    let (mut reader, writer) = UnixStream::pair()?;
    let mut child = command.stdout(Stdio::from(OwnedFd::from(writer))).stderr(Stdio::null()).stdin(Stdio::null()).spawn()?;
    drop(command);
    let deadline = Instant::now() + timeout;
    let result = (|| {
        let mut output = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let remaining = deadline.checked_duration_since(Instant::now()).ok_or(io::ErrorKind::TimedOut)?;
            reader.set_read_timeout(Some(remaining.max(Duration::from_millis(1))))?;
            let count = reader.read(&mut buffer)?;
            if count == 0 { break; }
            if output.len() + count > limit { return Err(io::Error::other("Fontconfig output exceeded limit")); }
            output.extend_from_slice(&buffer[..count]);
        }
        loop {
            if let Some(status) = child.try_wait()? {
                return if status.success() { Ok(output) } else { Err(io::Error::other("Fontconfig query failed")) };
            }
            if Instant::now() >= deadline { return Err(io::ErrorKind::TimedOut.into()); }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn dedup_fonts(fonts: Vec<FontInfo>) -> Vec<FontInfo> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped: Vec<FontInfo> = Vec::with_capacity(fonts.len());
    for font in fonts {
        let key = font.family.to_ascii_lowercase();
        if seen.insert(key) {
            deduped.push(font);
        }
    }
    deduped
}

fn sort_fonts(mut fonts: Vec<FontInfo>) -> Vec<FontInfo> {
    fonts.sort_by(|a, b| {
        a.family
            .to_ascii_lowercase()
            .cmp(&b.family.to_ascii_lowercase())
    });
    fonts
}

pub fn list_system_fonts() -> Vec<FontInfo> {
    list_system_fonts_with_source(&InstalledFonts)
}

fn list_system_fonts_with_source(source: &dyn FontSource) -> Vec<FontInfo> {
    source.all_fonts().unwrap_or_else(|error| {
        eprintln!("[fonts] System font enumeration failed: {error}");
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "linux", not(feature = "native-fonts")))]
    #[test]
    fn fontconfig_child_cannot_hold_the_worker_or_flood_its_output() {
        use std::{process::Command, time::{Duration, Instant}};
        let mut hung = Command::new("sleep");
        hung.arg("30");
        let started = Instant::now();
        assert!(fontconfig_output(hung, Duration::from_millis(100), 4096).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(fontconfig_output(Command::new("yes"), Duration::from_secs(3), 4096).is_err());
    }

    fn info(family: &str, mono: bool) -> FontInfo {
        FontInfo {
            family: family.to_string(),
            is_mono: mono,
            path: format!("/usr/share/fonts/{}.ttf", family),
        }
    }

    #[test]
    fn test_font_info_dedup() {
        let fonts = vec![
            info("Arial", false),
            info("arial", false),
            info("ARIAL", true),
            info("Helvetica", false),
        ];
        let deduped = dedup_fonts(fonts);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].family, "Arial");
        assert_eq!(deduped[0].is_mono, false, "first occurrence (regular) wins");
        assert_eq!(deduped[1].family, "Helvetica");
    }

    #[test]
    fn test_font_info_sort() {
        let fonts = vec![info("Zed", false), info("alpha", false), info("Mike", false)];
        let sorted = sort_fonts(fonts);
        assert_eq!(sorted[0].family, "alpha");
        assert_eq!(sorted[1].family, "Mike");
        assert_eq!(sorted[2].family, "Zed");
    }

    struct MockFailingSource;

    impl FontSource for MockFailingSource {
        fn all_fonts(&self) -> Result<Vec<FontInfo>, String> {
            Err("simulated fontconfig failure".to_string())
        }
    }

    #[test]
    fn test_list_system_fonts_handles_source_failure() {
        let mock = MockFailingSource;
        let result = list_system_fonts_with_source(&mock);
        assert!(
            result.is_empty(),
            "failed source should yield empty list"
        );
    }

    struct MockSource;

    impl FontSource for MockSource {
        fn all_fonts(&self) -> Result<Vec<FontInfo>, String> {
            Ok(vec![info("Worker Sans", false)])
        }
    }

    #[test]
    fn test_list_system_fonts_returns_source_fonts() {
        let result = list_system_fonts_with_source(&MockSource);
        assert_eq!(result[0].family, "Worker Sans");
    }
}
