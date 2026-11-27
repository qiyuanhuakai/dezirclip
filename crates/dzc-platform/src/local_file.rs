pub use dzc_core::filesystem::*;

/// Automatic previews must not open remote shares or redirected paths.
pub fn open_preview_file(path: &std::path::Path) -> Option<std::fs::File> {
    use std::path::Component;
    if !path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir)) { return None; }
    #[cfg(target_os = "windows")]
    {
        use std::{os::windows::fs::MetadataExt, path::Prefix};
        use windows::{core::PCWSTR, Win32::Storage::FileSystem::GetDriveTypeW};
        let Some(Component::Prefix(prefix)) = path.components().next() else { return None; };
        let drive = match prefix.kind() { Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter, _ => return None };
        let root = [u16::from(drive), b':' as u16, b'\\' as u16, 0];
        if !matches!(unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }, 2 | 3 | 5 | 6) { return None; }
        for ancestor in path.ancestors() {
            if std::fs::symlink_metadata(ancestor).ok()?.file_attributes() & 0x400 != 0 { return None; }
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    for ancestor in path.ancestors() {
        if std::fs::symlink_metadata(ancestor).ok()?.file_type().is_symlink() { return None; }
    }
    crate::local_file::open_regular_nofollow(path).ok()
}

pub(crate) fn read_preview_file(path: &std::path::Path, maximum: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = open_preview_file(path)?;
    let before = file.metadata().ok()?;
    if before.len() > maximum.min(16 * 1024 * 1024) as u64 { return None; }
    let mut bytes = Vec::new();
    (&file).take(maximum.min(16 * 1024 * 1024) as u64 + 1).read_to_end(&mut bytes).ok()?;
    let after = file.metadata().ok()?;
    (bytes.len() as u64 == before.len() && before.len() == after.len() && before.modified().ok()? == after.modified().ok()?).then_some(bytes)
}
