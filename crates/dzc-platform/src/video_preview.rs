//! Decode one frame in a short-lived helper; codec faults cannot kill the host.
use std::{io::{Read, Write}, process::{Command, Stdio}, time::{Duration, Instant}};
const MAX_INPUT: usize = 16 * 1024 * 1024;
const MAX_OUTPUT: usize = 2 * 1024 * 1024;
const FLAG: &str = "--dzc-video-preview";

pub fn install() { dzc_core::media_preview::install_video_decoder(first_frame); }
pub fn dispatch() {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(FLAG)) { return; }
    let result = (|| -> Option<()> {
        let mut input = Vec::new();
        std::io::stdin().take((MAX_INPUT + 5) as u64).read_to_end(&mut input).ok()?;
        if input.len() < 5 || input.len() > MAX_INPUT + 4 { return None; }
        if !supported_container(&input[4..]) { return None; }
        let maximum = u32::from_le_bytes(input[..4].try_into().ok()?).clamp(64, 560);
        let png = decode(&input[4..], maximum)?;
        if png.len() > MAX_OUTPUT { return None; }
        std::io::stdout().write_all(&png).ok()?;
        Some(())
    })();
    std::process::exit(if result.is_some() { 0 } else { 1 });
}

fn supported_container(bytes: &[u8]) -> bool {
    // Reject playlists and URL redirection before invoking any system codec.
    bytes.get(4..8) == Some(b"ftyp") || bytes.get(4..8) == Some(b"moov")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"AVI "))
        || bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        || bytes.starts_with(&[0x30, 0x26, 0xb2, 0x75, 0x8e, 0x66, 0xcf, 0x11, 0xa6, 0xd9, 0x00, 0xaa, 0x00, 0x62, 0xce, 0x6c])
}

pub fn first_frame(bytes: &[u8], maximum: u32) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT { return None; }
    let mut command = Command::new(std::env::current_exe().ok()?);
    command.arg(FLAG).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    #[cfg(target_os = "linux")]
    { use std::os::unix::process::CommandExt; command.process_group(0); }
    let mut child = command.spawn().ok()?;
    let mut input = child.stdin.take()?;
    let output = child.stdout.take()?;
    let bytes = bytes.to_vec();
    let writer = std::thread::spawn(move || { let _ = input.write_all(&maximum.to_le_bytes()); let _ = input.write_all(&bytes); });
    let reader = std::thread::spawn(move || { let mut bytes = Vec::new(); let _ = output.take((MAX_OUTPUT + 1) as u64).read_to_end(&mut bytes); bytes });
    let started = Instant::now();
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < Duration::from_secs(3) => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                #[cfg(target_os = "linux")]
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
                let _ = child.kill(); let _ = child.wait(); break false;
            }
        }
    };
    let _ = writer.join();
    let png = reader.join().ok()?;
    (success && png.len() <= MAX_OUTPUT && png.starts_with(b"\x89PNG\r\n\x1a\n")).then_some(png)
}

#[cfg(test)]
mod tests {
    #[test]
    fn decoder_rejects_empty_and_oversized_inputs_before_spawning() {
        assert!(super::first_frame(&[], 384).is_none());
        assert!(super::first_frame(&vec![0; super::MAX_INPUT + 1], 384).is_none());
        assert!(!super::supported_container(b"<ASX><ENTRY><REF HREF='https://example.invalid/movie' /></ENTRY></ASX>"));
    }
}

#[cfg(target_os = "windows")]
fn decode(bytes: &[u8], maximum: u32) -> Option<Vec<u8>> {
    use windows::Win32::{Media::MediaFoundation::*, System::Com::{CoInitializeEx, COINIT_MULTITHREADED}, UI::Shell::SHCreateMemStream};
    // This function only runs in the disposable decoder process. All COM and
    // Media Foundation resources are released when that process exits.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
        MFStartup(MF_VERSION, MFSTARTUP_FULL).ok()?;
        let stream = SHCreateMemStream(Some(bytes))?;
        let stream = MFCreateMFByteStreamOnStream(&stream).ok()?;
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1).ok()?;
        let attrs = attrs?;
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).ok()?;
        let reader = MFCreateSourceReaderFromByteStream(&stream, &attrs).ok()?;
        let video = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).ok()?;
        reader.SetStreamSelection(video, true).ok()?;
        let native = reader.GetNativeMediaType(video, 0).ok()?;
        let dimensions = native.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
        let (w, h) = ((dimensions >> 32) as u32, dimensions as u32);
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16 * 1024 * 1024 { return None; }
        let format = MFCreateMediaType().ok()?;
        format.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
        format.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok()?;
        reader.SetCurrentMediaType(video, None, &format).ok()?;
        for _ in 0..64 {
            let mut sample = None;
            let mut flags = 0;
            reader.ReadSample(video, 0, None, Some(&mut flags), None, Some(&mut sample)).ok()?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 { return None; }
            let Some(sample) = sample else { continue; };
            let format = reader.GetCurrentMediaType(video).ok()?;
            let dimensions = format.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
            let (w, h) = ((dimensions >> 32) as u32, dimensions as u32);
            if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16 * 1024 * 1024 { return None; }
            let stride = format.GetUINT32(&MF_MT_DEFAULT_STRIDE).unwrap_or(w * 4) as i32;
            let pitch = stride.unsigned_abs() as usize;
            let buffer = sample.ConvertToContiguousBuffer().ok()?;
            let mut pointer = std::ptr::null_mut();
            let mut length = 0;
            buffer.Lock(&mut pointer, None, Some(&mut length)).ok()?;
            if pointer.is_null() || pitch < w as usize * 4 || pitch.checked_mul(h as usize)? > length as usize {
                let _ = buffer.Unlock(); return None;
            }
            let raw = std::slice::from_raw_parts(pointer, length as usize);
            let mut image = image::RgbaImage::new(w, h);
            for y in 0..h {
                let row = if stride < 0 { h - 1 - y } else { y } as usize * pitch;
                for x in 0..w {
                    let i = row + x as usize * 4;
                    image.put_pixel(x, y, image::Rgba([raw[i + 2], raw[i + 1], raw[i], 255]));
                }
            }
            let _ = buffer.Unlock();
            let image = image::DynamicImage::ImageRgba8(image).thumbnail(maximum, maximum);
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, image::ImageFormat::Png).ok()?;
            return Some(png.into_inner());
        }
        None
    }
}

#[cfg(target_os = "linux")]
fn decode(bytes: &[u8], maximum: u32) -> Option<Vec<u8>> {
    // ffmpeg stays inside this isolated helper's lifetime. Only pipe input is
    // enabled, so a crafted playlist cannot fetch remote or other local files.
    let mut child = Command::new("ffmpeg").args(["-v", "error", "-protocol_whitelist", "pipe", "-i", "pipe:0", "-frames:v", "1", "-vf",
        &format!("scale={maximum}:{maximum}:force_original_aspect_ratio=decrease"), "-f", "image2pipe", "-vcodec", "png", "pipe:1"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut input = child.stdin.take()?;
    let bytes = bytes.to_vec();
    let writer = std::thread::spawn(move || { let _ = input.write_all(&bytes); });
    let mut png = Vec::new();
    child.stdout.take()?.take((MAX_OUTPUT + 1) as u64).read_to_end(&mut png).ok()?;
    let _ = writer.join();
    if png.len() > MAX_OUTPUT { let _ = child.kill(); }
    child.wait().ok()?.success().then_some(png)
}
