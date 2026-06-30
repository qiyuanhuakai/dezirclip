use crate::error::AppResult;
use font_kit::handle::Handle;
use font_kit::source::SystemSource;

#[derive(Debug, Clone, serde::Serialize)]
pub struct FontInfo {
    pub family: String,
    pub is_mono: bool,
    pub path: String,
}

pub trait FontSource {
    fn all_fonts(&self) -> Result<Vec<FontInfo>, String>;
}

pub struct SystemSourceWrapper {
    inner: SystemSource,
}

impl SystemSourceWrapper {
    pub fn new() -> Self {
        Self {
            inner: SystemSource::new(),
        }
    }
}

impl FontSource for SystemSourceWrapper {
    fn all_fonts(&self) -> Result<Vec<FontInfo>, String> {
        let handles = self
            .inner
            .all_fonts()
            .map_err(|e| format!("system source enumeration failed: {e}"))?;

        let mut fonts: Vec<FontInfo> = Vec::with_capacity(handles.len());
        for handle in handles {
            match &handle {
                Handle::Path { path, .. } => {
                    let font = match handle.load() {
                        Ok(f) => f,
                        Err(e) => {
                            crate::warn!(
                                "[font_cmd] skipping font at {} (load failed: {})",
                                path.display(),
                                e
                            );
                            continue;
                        }
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

#[tauri::command]
pub async fn list_system_fonts() -> AppResult<Vec<FontInfo>> {
    list_system_fonts_with_source_factory(SystemSourceWrapper::new).await
}

async fn list_system_fonts_with_source_factory<F, T>(source_factory: F) -> AppResult<Vec<FontInfo>>
where
    F: FnOnce() -> T + Send + 'static,
    T: FontSource,
{
    tauri::async_runtime::spawn_blocking(move || {
        let source = source_factory();
        list_system_fonts_with_source(&source)
    })
    .await
    .map_err(|e| format!("font enumeration worker failed: {e}"))?
}

fn list_system_fonts_with_source(source: &dyn FontSource) -> AppResult<Vec<FontInfo>> {
    match source.all_fonts() {
        Ok(fonts) => Ok(fonts),
        Err(e) => {
            crate::warn!("[font_cmd] list_system_fonts returned empty: {}", e);
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(result.is_ok(), "graceful failure should still return Ok");
        assert!(
            result.unwrap().is_empty(),
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
    fn test_list_system_fonts_blocking_uses_owned_source() {
        let result =
            tauri::async_runtime::block_on(list_system_fonts_with_source_factory(|| MockSource));

        assert!(result.is_ok(), "blocking worker should return source fonts");
        assert_eq!(result.unwrap()[0].family, "Worker Sans");
    }
}
