//! Short clipboard cues. No audio device or playback thread stays open between bursts.
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Copy,
    Paste,
}

#[derive(Default)]
struct Playback {
    running: bool,
    next: Option<(Cue, u8)>,
}

static PLAYBACK: Mutex<Playback> = Mutex::new(Playback { running: false, next: None });
const RATE: u32 = 22050;

pub fn play(history: &dzc_core::api::HistoryService, cue: Cue) {
    let Ok(volume) = history.feedback_volume(cue == Cue::Paste) else { return; };
    if volume == 0 { return; }
    let Ok(mut state) = PLAYBACK.lock() else { return; };
    if state.running {
        state.next = Some((cue, volume));
        return;
    }
    state.running = true;
    if std::thread::Builder::new().name("clipboard-sound".into()).spawn(move || {
        let mut current = (cue, volume);
        loop {
            output(&samples(current.0, current.1));
            let Ok(mut state) = PLAYBACK.lock() else { return; };
            match state.next.take() {
                Some(next) => current = next,
                None => { state.running = false; return; }
            }
        }
    }).is_err() {
        state.running = false;
    }
}

fn samples(cue: Cue, volume: u8) -> Vec<i16> {
    let notes: &[(u32, u32, f32)] = match cue {
        Cue::Copy => &[(0, 60, 500.)],
        Cue::Paste => &[(0, 90, 950.), (110, 75, 1150.)],
    };
    let mut pcm = Vec::new();
    for &(offset_ms, duration_ms, frequency) in notes {
        pcm.resize((offset_ms * RATE / 1000) as usize, 0);
        let count = duration_ms * RATE / 1000;
        let mut phase = 0.0_f32;
        for i in 0..count {
            let t = i as f32 / count as f32;
            phase += frequency * (1.25 - 0.33 * (4. * t).min(1.)) / RATE as f32;
            let triangle = 4. * (phase.fract() - 0.5).abs() - 1.;
            let envelope = (i as f32 / (0.004 * RATE as f32)).min(1.) * (1. - t).powi(4);
            pcm.push((triangle * envelope * volume.min(100) as f32 * 140.) as i16);
        }
    }
    pcm
}

#[cfg(target_os = "windows")]
fn output(samples: &[i16]) {
    #[link(name = "winmm")]
    extern "system" { fn PlaySoundW(sound: *const u8, module: usize, flags: u32) -> i32; }
    let size = samples.len() as u32 * 2;
    let mut wave = Vec::with_capacity(size as usize + 44);
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(size + 36).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    wave.extend_from_slice(&RATE.to_le_bytes());
    wave.extend_from_slice(&(RATE * 2).to_le_bytes());
    wave.extend_from_slice(b"\x02\0\x10\0data");
    wave.extend_from_slice(&size.to_le_bytes());
    wave.extend(samples.iter().flat_map(|n| n.to_le_bytes()));
    // SND_MEMORY | SND_NODEFAULT, synchronous so wave outlives playback.
    unsafe { PlaySoundW(wave.as_ptr(), 0, 6); }
}

#[cfg(target_os = "linux")]
fn output(samples: &[i16]) {
    use libloading::Library;
    use std::{ffi::{c_char, c_int, c_void}, ptr};
    #[repr(C)]
    struct SampleSpec { format: c_int, rate: u32, channels: u8 }
    type Open = unsafe extern "C" fn(*const c_char, *const c_char, c_int, *const c_char,
        *const c_char, *const SampleSpec, *const c_void, *const c_void, *mut c_int) -> *mut c_void;
    type Write = unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int;
    type Drain = unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int;
    type Free = unsafe extern "C" fn(*mut c_void);
    // Optional PulseAudio/PipeWire client. Its handle and symbol references remain
    // live until the stream is freed. Playback and drain only block the cue worker.
    unsafe {
        let Ok(lib) = Library::new("libpulse-simple.so.0") else { return; };
        let (Ok(open), Ok(write), Ok(drain), Ok(free)) = (lib.get::<Open>(b"pa_simple_new"),
            lib.get::<Write>(b"pa_simple_write"), lib.get::<Drain>(b"pa_simple_drain"),
            lib.get::<Free>(b"pa_simple_free")) else { return; };
        let spec = SampleSpec { format: if cfg!(target_endian = "little") { 3 } else { 4 }, rate: RATE, channels: 1 };
        let stream = open(ptr::null(), c"DezirClip".as_ptr(), 1, ptr::null(), c"Clipboard".as_ptr(),
            &spec, ptr::null(), ptr::null(), ptr::null_mut());
        if !stream.is_null() {
            if write(stream, samples.as_ptr().cast(), std::mem::size_of_val(samples), ptr::null_mut()) == 0 {
                drain(stream, ptr::null_mut());
            }
            free(stream);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cue_lengths_pause_and_volume_follow_the_desktop_settings() {
        let copy = samples(Cue::Copy, 100);
        assert_eq!(copy.len(), (60 * RATE / 1000) as usize);
        let quiet = samples(Cue::Copy, 20);
        assert!(quiet.iter().map(|n| n.abs()).max().unwrap() * 4 < copy.iter().map(|n| n.abs()).max().unwrap());
        let paste = samples(Cue::Paste, 70);
        assert!(paste[(90 * RATE / 1000) as usize..(110 * RATE / 1000) as usize].iter().all(|&s| s == 0));
        assert!(paste[(110 * RATE / 1000) as usize..].iter().any(|&s| s != 0));
    }
}
