//! Terminal ownership for the standalone history browser.

use std::fs::{File, OpenOptions};
use std::io::{IsTerminal, Write};

/// Rendering and input use the controlling terminal independently of stdin/stdout.
pub struct Terminal {
    output: File,
    _input: File,
    raw: bool,
    active: bool,
    #[cfg(target_os = "windows")]
    output_mode: Option<OutputMode>,
    #[cfg(target_os = "windows")]
    input_mode: Option<OutputMode>,
    #[cfg(target_os = "windows")]
    code_pages: Option<CodePages>,
    #[cfg(target_os = "windows")]
    decoder: vt_input::Decoder,
    #[cfg(target_os = "windows")]
    pending: std::collections::VecDeque<u8>,
    #[cfg(target_os = "windows")]
    input_error: Option<String>,
    #[cfg(target_os = "windows")]
    unicode_input: vt_input::Utf16Decoder,
}

impl Terminal {
    pub fn enter() -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let (input_name, output_name) = ("/dev/tty", "/dev/tty");
        #[cfg(target_os = "windows")]
        let (input_name, output_name) = ("CONIN$", "CONOUT$");
        let open = |name| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(name)
                .map_err(|error| format!("tui requires a controlling terminal ({name}): {error}; run in an interactive terminal or an SSH session with a PTY"))
        };
        let input = open(input_name)?;
        let output = open(output_name)?;
        if !input.is_terminal() || !output.is_terminal() {
            return Err("tui requires a controlling terminal".to_string());
        }
        #[cfg(target_os = "windows")]
        let code_pages = CodePages::enable_utf8()?;
        #[cfg(target_os = "windows")]
        let output_mode = OutputMode::enable(&output)?;
        #[cfg(target_os = "windows")]
        let input_mode = OutputMode::save(&input)?;
        let mut terminal = Self {
            output,
            _input: input,
            raw: false,
            active: false,
            #[cfg(target_os = "windows")]
            output_mode: Some(output_mode),
            #[cfg(target_os = "windows")]
            input_mode: Some(input_mode),
            #[cfg(target_os = "windows")]
            code_pages: Some(code_pages),
            #[cfg(target_os = "windows")]
            decoder: vt_input::Decoder::default(),
            #[cfg(target_os = "windows")]
            pending: std::collections::VecDeque::new(),
            #[cfg(target_os = "windows")]
            input_error: None,
            #[cfg(target_os = "windows")]
            unicode_input: vt_input::Utf16Decoder::default(),
        };
        crossterm::terminal::enable_raw_mode()
            .map_err(|error| format!("tui cannot enable terminal input: {error}"))?;
        terminal.raw = true;
        #[cfg(target_os = "windows")]
        terminal.enable_vt_input()?;
        // Mark active before writing so partial writes also trigger restoration.
        terminal.active = true;
        terminal.write("\x1b[?1049h\x1b[?25l\x1b[?2004h")?;
        Ok(terminal)
    }

    pub fn write(&mut self, frame: &str) -> Result<(), String> {
        self.output
            .write_all(frame.as_bytes())
            .and_then(|()| self.output.flush())
            .map_err(|error| format!("tui terminal write failed: {error}"))
    }

    pub fn output_clone(&self) -> Result<File, String> {
        self.output
            .try_clone()
            .map_err(|error| format!("tui cannot duplicate terminal output: {error}"))
    }

    pub fn size(&self) -> Result<(u16, u16), String> {
        #[cfg(target_os = "linux")]
        let result = crossterm::terminal::size();
        #[cfg(target_os = "windows")]
        let result = {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::Console::{
                GetConsoleScreenBufferInfo, CONSOLE_SCREEN_BUFFER_INFO,
            };
            let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
            unsafe { GetConsoleScreenBufferInfo(HANDLE(self.output.as_raw_handle()), &mut info) }
                .map(|()| {
                    (
                        (info.srWindow.Right - info.srWindow.Left + 1) as u16,
                        (info.srWindow.Bottom - info.srWindow.Top + 1) as u16,
                    )
                })
                .map_err(std::io::Error::other)
        };
        result.map_err(|error| format!("tui cannot read terminal size: {error}"))
    }

    #[cfg(target_os = "linux")]
    pub fn read(&mut self) -> Result<crossterm::event::Event, String> {
        crossterm::event::read().map_err(|error| format!("tui terminal input failed: {error}"))
    }

    #[cfg(target_os = "windows")]
    fn enable_vt_input(&self) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Console::{
            GetConsoleMode, SetConsoleMode, CONSOLE_MODE, ENABLE_EXTENDED_FLAGS,
            ENABLE_MOUSE_INPUT, ENABLE_QUICK_EDIT_MODE, ENABLE_VIRTUAL_TERMINAL_INPUT,
            ENABLE_WINDOW_INPUT,
        };
        let handle = HANDLE(self._input.as_raw_handle());
        let mut mode = CONSOLE_MODE::default();
        unsafe {
            GetConsoleMode(handle, &mut mode)
                .and_then(|()| SetConsoleMode(handle, (mode | ENABLE_VIRTUAL_TERMINAL_INPUT | ENABLE_EXTENDED_FLAGS)
                    & !(ENABLE_MOUSE_INPUT | ENABLE_WINDOW_INPUT | ENABLE_QUICK_EDIT_MODE)))
                .map_err(|error| format!("tui requires Windows virtual terminal input for safe paste handling: {error}; use Windows Terminal or a ConPTY terminal"))
        }
    }

    #[cfg(target_os = "windows")]
    pub fn read(&mut self) -> Result<crossterm::event::Event, String> {
        if let Some(error) = &self.input_error {
            return Err(error.clone());
        }
        let result = self.read_vt();
        if let Err(error) = &result {
            self.discard_pending_input();
            self.input_error = Some(error.clone());
        }
        result
    }

    #[cfg(target_os = "windows")]
    fn read_vt(&mut self) -> Result<crossterm::event::Event, String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Console::ReadConsoleW;
        let size = self.size()?;
        loop {
            while let Some(byte) = self.pending.pop_front() {
                match self.decoder.push(byte) {
                    Ok(Some(event)) => return Ok(event),
                    Ok(None) => {}
                    Err(error) => return Err(error),
                }
            }
            // Only a lone Escape key has an ambiguity timeout. Partial CSI,
            // UTF-8 and recognized paste frames remain data across every read.
            let timeout = if self.decoder.is_lone_escape() {
                100
            } else {
                250
            };
            if !self.input_ready(timeout)? {
                if let Some(event) = self.decoder.finish_escape() {
                    return Ok(event);
                }
                let current_size = self.size()?;
                if current_size != size {
                    return Ok(crossterm::event::Event::Resize(
                        current_size.0,
                        current_size.1,
                    ));
                }
                continue;
            }
            let mut units = [0u16; 1024];
            let mut count = 0;
            unsafe {
                ReadConsoleW(
                    HANDLE(self._input.as_raw_handle()),
                    units.as_mut_ptr().cast(),
                    units.len() as u32,
                    &mut count,
                    None,
                )
            }
            .map_err(|error| format!("tui terminal input failed: {error}"))?;
            if count == 0 {
                return Err("tui terminal input closed before an input event completed".to_string());
            }
            for &unit in &units[..count as usize] {
                if let Some(character) = self.unicode_input.push(unit)? {
                    let mut bytes = [0; 4];
                    self.pending
                        .extend(character.encode_utf8(&mut bytes).as_bytes());
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn input_ready(&self, timeout: u32) -> Result<bool, String> {
        use std::os::windows::io::AsRawHandle;
        use std::time::{Duration, Instant};
        use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Console::{
            PeekConsoleInputW, ReadConsoleInputW, INPUT_RECORD, KEY_EVENT,
        };
        use windows::Win32::System::Threading::WaitForSingleObject;
        let handle = HANDLE(self._input.as_raw_handle());
        let deadline = Instant::now() + Duration::from_millis(u64::from(timeout));
        loop {
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u32;
            let waited = unsafe { WaitForSingleObject(handle, remaining) };
            if waited == WAIT_TIMEOUT {
                return Ok(false);
            }
            if waited != WAIT_OBJECT_0 {
                return Err("tui cannot wait for console input".to_string());
            }
            let mut records = [INPUT_RECORD::default()];
            let mut count = 0;
            unsafe { PeekConsoleInputW(handle, &mut records, &mut count) }
                .map_err(|error| format!("tui cannot inspect console input: {error}"))?;
            if count == 0 {
                continue;
            }
            let record = records[0];
            if u32::from(record.EventType) == KEY_EVENT {
                let key = unsafe { record.Event.KeyEvent };
                // VT text uses key-down records; Unicode input can also use
                // key-up surrogate records. Keep both without waiting on
                // ordinary modifier bookkeeping that has no readable text.
                let unicode = unsafe { key.uChar.UnicodeChar };
                if (!key.bKeyDown.as_bool() && unicode >= 0x80)
                    || (key.bKeyDown.as_bool()
                        && (unicode != 0
                            || matches!(key.wVirtualKeyCode, 0x08 | 0x09 | 0x0d | 0x1b | 0x21..=0x28 | 0x2d | 0x2e | 0x70..=0x7b)))
                {
                    return Ok(true);
                }
            }
            unsafe { ReadConsoleInputW(handle, &mut records, &mut count) }
                .map_err(|error| format!("tui cannot discard console bookkeeping: {error}"))?;
        }
    }

    #[cfg(target_os = "windows")]
    fn discard_pending_input(&mut self) {
        use std::os::windows::io::AsRawHandle;
        self.pending.clear();
        unsafe {
            let _ = windows::Win32::System::Console::FlushConsoleInputBuffer(
                windows::Win32::Foundation::HANDLE(self._input.as_raw_handle()),
            );
        }
    }

    pub fn restore(&mut self) -> Result<(), String> {
        let screen_result = if self.active {
            self.active = false;
            self.write("\x1b[0m\x1b[?2004l\x1b[?25h\x1b[?1049l")
        } else {
            Ok(())
        };
        let raw_result = if self.raw {
            self.raw = false;
            crossterm::terminal::disable_raw_mode()
                .map_err(|error| format!("tui cannot restore terminal input: {error}"))
        } else {
            Ok(())
        };
        #[cfg(target_os = "windows")]
        drop(self.output_mode.take());
        #[cfg(target_os = "windows")]
        drop(self.input_mode.take());
        #[cfg(target_os = "windows")]
        drop(self.code_pages.take());
        screen_result.and(raw_result)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(target_os = "windows")]
struct OutputMode {
    handle: windows::Win32::Foundation::HANDLE,
    previous: windows::Win32::System::Console::CONSOLE_MODE,
}

#[cfg(target_os = "windows")]
impl OutputMode {
    fn save(file: &File) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Console::{GetConsoleMode, CONSOLE_MODE};
        let handle = HANDLE(file.as_raw_handle());
        let mut previous = CONSOLE_MODE::default();
        unsafe { GetConsoleMode(handle, &mut previous) }
            .map_err(|error| format!("tui cannot inspect console mode: {error}"))?;
        Ok(Self { handle, previous })
    }

    fn enable(output: &File) -> Result<Self, String> {
        use windows::Win32::System::Console::{
            SetConsoleMode, ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        };
        let mode = Self::save(output)?;
        unsafe {
            SetConsoleMode(
                mode.handle,
                mode.previous | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
            )
            .map_err(|error| format!("tui requires virtual terminal support: {error}"))?;
        }
        Ok(mode)
    }
}

#[cfg(target_os = "windows")]
impl Drop for OutputMode {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Console::SetConsoleMode(self.handle, self.previous);
        }
    }
}

#[cfg(target_os = "windows")]
struct CodePages {
    input: u32,
    output: u32,
}

#[cfg(target_os = "windows")]
impl CodePages {
    fn enable_utf8() -> Result<Self, String> {
        use windows::Win32::System::Console::{
            GetConsoleCP, GetConsoleOutputCP, SetConsoleCP, SetConsoleOutputCP,
        };
        let previous = unsafe {
            Self {
                input: GetConsoleCP(),
                output: GetConsoleOutputCP(),
            }
        };
        unsafe {
            SetConsoleOutputCP(65001)
                .and_then(|()| SetConsoleCP(65001))
                .map_err(|error| format!("tui cannot enable UTF-8 console text: {error}"))?;
        }
        Ok(previous)
    }
}

#[cfg(target_os = "windows")]
impl Drop for CodePages {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Console::SetConsoleCP(self.input);
            let _ = windows::Win32::System::Console::SetConsoleOutputCP(self.output);
        }
    }
}

#[cfg(any(target_os = "windows", test))]
mod vt_input {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    pub(super) const MAX_PASTE_BYTES: usize = 64 * 1024;
    const PASTE_END: &[u8] = b"\x1b[201~";

    #[derive(Default)]
    pub(super) struct Utf16Decoder {
        high: Option<u16>,
    }

    impl Utf16Decoder {
        pub(super) fn push(&mut self, unit: u16) -> Result<Option<char>, String> {
            if let Some(high) = self.high.take() {
                if !(0xdc00..=0xdfff).contains(&unit) {
                    return Err("tui received an incomplete Unicode surrogate pair".to_string());
                }
                return Ok(char::from_u32(
                    0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(unit) - 0xdc00,
                ));
            }
            if (0xd800..=0xdbff).contains(&unit) {
                self.high = Some(unit);
                Ok(None)
            } else {
                char::from_u32(u32::from(unit))
                    .map(Some)
                    .ok_or_else(|| "tui received an invalid Unicode surrogate".to_string())
            }
        }
    }

    #[derive(Default)]
    pub(super) struct Decoder {
        state: State,
    }

    #[derive(Default)]
    enum State {
        #[default]
        Ground,
        Escape(Vec<u8>),
        Utf8(Vec<u8>, usize, KeyModifiers),
        StringControl(bool, bool),
        Paste(Paste),
    }

    #[derive(Default)]
    struct Paste {
        content: Vec<u8>,
        marker: Vec<u8>,
        oversized: bool,
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Option<Event> {
        Some(Event::Key(KeyEvent::new(code, modifiers)))
    }

    impl Decoder {
        pub(super) fn is_lone_escape(&self) -> bool {
            matches!(&self.state, State::Escape(bytes) if bytes == b"\x1b")
        }

        pub(super) fn finish_escape(&mut self) -> Option<Event> {
            if !self.is_lone_escape() {
                return None;
            }
            self.state = State::Ground;
            key(KeyCode::Esc, KeyModifiers::NONE)
        }

        pub(super) fn push(&mut self, byte: u8) -> Result<Option<Event>, String> {
            if byte == 3 && !matches!(self.state, State::Ground | State::Paste(_)) {
                return Err("tui cancelled an incomplete terminal control sequence".to_string());
            }
            match std::mem::take(&mut self.state) {
                State::Ground => self.character(byte, KeyModifiers::NONE),
                State::Utf8(mut bytes, expected, modifiers) => {
                    bytes.push(byte);
                    if bytes.len() == expected {
                        let text = std::str::from_utf8(&bytes)
                            .map_err(|_| "tui received invalid UTF-8 terminal input")?;
                        Ok(key(KeyCode::Char(text.chars().next().unwrap()), modifiers))
                    } else {
                        self.state = State::Utf8(bytes, expected, modifiers);
                        Ok(None)
                    }
                }
                State::Escape(mut bytes) => {
                    if bytes.len() == 1 {
                        match byte {
                            b'[' | b'O' => {
                                bytes.push(byte);
                                self.state = State::Escape(bytes);
                                return Ok(None);
                            }
                            b']' => {
                                self.state = State::StringControl(true, false);
                                return Ok(None);
                            }
                            b'P' | b'_' | b'^' | b'X' => {
                                self.state = State::StringControl(false, false);
                                return Ok(None);
                            }
                            _ => return self.character(byte, KeyModifiers::ALT),
                        }
                    }
                    bytes.push(byte);
                    if bytes.len() > 64 {
                        return Err(
                            "tui terminal escape sequence exceeds the 64-byte limit".to_string()
                        );
                    }
                    if !(0x40..=0x7e).contains(&byte) {
                        self.state = State::Escape(bytes);
                        return Ok(None);
                    }
                    if bytes == b"\x1b[200~" {
                        self.state = State::Paste(Paste::default());
                        return Ok(None);
                    }
                    // Unknown sequences, including a stray paste terminator,
                    // are consumed whole and never replayed as shortcut keys.
                    Ok(decode_sequence(&bytes))
                }
                State::StringControl(osc, escaped) => {
                    if (osc && byte == 7) || (escaped && byte == b'\\') {
                        Ok(None)
                    } else {
                        self.state = State::StringControl(osc, byte == 0x1b);
                        Ok(None)
                    }
                }
                State::Paste(mut paste) => {
                    paste.marker.push(byte);
                    while !PASTE_END.starts_with(&paste.marker) {
                        let content_byte = paste.marker.remove(0);
                        if paste.content.len() < MAX_PASTE_BYTES {
                            paste.content.push(content_byte);
                        } else {
                            paste.oversized = true;
                        }
                    }
                    if paste.marker == PASTE_END {
                        if paste.oversized {
                            return Err(format!("tui pasted input exceeds the {MAX_PASTE_BYTES}-byte limit; paste was discarded"));
                        }
                        let text = String::from_utf8(paste.content)
                            .map_err(|_| "tui pasted input is not valid UTF-8")?;
                        Ok(Some(Event::Paste(text)))
                    } else {
                        self.state = State::Paste(paste);
                        Ok(None)
                    }
                }
            }
        }

        fn character(
            &mut self,
            byte: u8,
            modifiers: KeyModifiers,
        ) -> Result<Option<Event>, String> {
            Ok(match byte {
                0x1b if modifiers.is_empty() => {
                    self.state = State::Escape(vec![byte]);
                    None
                }
                0x1b => key(KeyCode::Esc, modifiers),
                b'\r' | b'\n' => key(KeyCode::Enter, modifiers),
                b'\t' => key(KeyCode::Tab, modifiers),
                8 | 127 => key(KeyCode::Backspace, modifiers),
                1..=26 => key(
                    KeyCode::Char(char::from(b'a' + byte - 1)),
                    modifiers | KeyModifiers::CONTROL,
                ),
                0 => None,
                0x20..=0x7e => key(KeyCode::Char(char::from(byte)), modifiers),
                0xc2..=0xf4 => {
                    let expected = if byte < 0xe0 {
                        2
                    } else if byte < 0xf0 {
                        3
                    } else {
                        4
                    };
                    self.state = State::Utf8(vec![byte], expected, modifiers);
                    None
                }
                _ => return Err("tui received invalid UTF-8 terminal input".to_string()),
            })
        }
    }

    fn decode_sequence(bytes: &[u8]) -> Option<Event> {
        let (&last, parameters) = bytes.get(2..)?.split_last()?;
        let parameters = std::str::from_utf8(parameters).ok()?;
        if !parameters.chars().all(|c| c.is_ascii_digit() || c == ';') {
            return None;
        }
        let parts: Vec<_> = parameters.split(';').collect();
        let modifier = if parts.len() > 1 {
            parts.last()?.parse::<u8>().ok()?.checked_sub(1)?
        } else {
            0
        };
        let mut modifiers = KeyModifiers::NONE;
        if modifier & 1 != 0 {
            modifiers |= KeyModifiers::SHIFT;
        }
        if modifier & 2 != 0 {
            modifiers |= KeyModifiers::ALT;
        }
        if modifier & 4 != 0 {
            modifiers |= KeyModifiers::CONTROL;
        }
        let code = match last {
            b'A' => KeyCode::Up,
            b'B' => KeyCode::Down,
            b'C' => KeyCode::Right,
            b'D' => KeyCode::Left,
            b'H' => KeyCode::Home,
            b'F' => KeyCode::End,
            b'~' => match *parts.first()? {
                "1" | "7" => KeyCode::Home,
                "4" | "8" => KeyCode::End,
                "5" => KeyCode::PageUp,
                "6" => KeyCode::PageDown,
                "3" => KeyCode::Delete,
                "2" => KeyCode::Insert,
                _ => return None,
            },
            _ => return None,
        };
        key(code, modifiers)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn decode(bytes: &[u8]) -> Result<Vec<Event>, String> {
            let mut decoder = Decoder::default();
            let mut events = Vec::new();
            for &byte in bytes {
                if let Some(event) = decoder.push(byte)? {
                    events.push(event);
                }
            }
            Ok(events)
        }

        #[test]
        fn fragmented_paste_keeps_all_controls_utf8_and_nested_escape_sequences_as_data() {
            let text = "hello\rdyes\r\n中文🙂\x03\x1b[A\x1b[200~\x1b]52;c;data\x07";
            let bytes = format!("\x1b[200~{text}\x1b[201~");
            assert_eq!(
                decode(bytes.as_bytes()).unwrap(),
                vec![Event::Paste(text.to_string())]
            );
            let mut decoder = Decoder::default();
            for &byte in b"\x1b[200~dyes\r\x1b[201" {
                assert!(decoder.push(byte).unwrap().is_none());
                if !decoder.is_lone_escape() {
                    assert!(decoder.finish_escape().is_none());
                }
            }
            assert_eq!(
                decoder.push(b'~').unwrap(),
                Some(Event::Paste("dyes\r".to_string()))
            );
        }

        #[test]
        fn paste_limit_is_exact_and_oversize_is_drained_before_error() {
            let boundary = format!("\x1b[200~{}\x1b[201~", "x".repeat(MAX_PASTE_BYTES));
            assert_eq!(
                decode(boundary.as_bytes()).unwrap(),
                vec![Event::Paste("x".repeat(MAX_PASTE_BYTES))]
            );
            let mut decoder = Decoder::default();
            for &byte in b"\x1b[200~"
                .iter()
                .chain(std::iter::repeat(&b'x').take(MAX_PASTE_BYTES + 100))
                .chain(b"\rdyes\r\x1b[201".iter())
            {
                assert!(decoder.push(byte).unwrap().is_none());
            }
            assert!(decoder
                .push(b'~')
                .unwrap_err()
                .contains("paste was discarded"));
        }

        #[test]
        fn navigation_and_utf8_are_decoded_without_unknown_sequence_fallthrough() {
            let events =
                decode("你🙂\x1b[A\x1bOB\x1b[1;5C\x1b[5~\x1b[6~\x1b[4~".as_bytes()).unwrap();
            assert_eq!(
                events,
                vec![
                    key(KeyCode::Char('你'), KeyModifiers::NONE).unwrap(),
                    key(KeyCode::Char('🙂'), KeyModifiers::NONE).unwrap(),
                    key(KeyCode::Up, KeyModifiers::NONE).unwrap(),
                    key(KeyCode::Down, KeyModifiers::NONE).unwrap(),
                    key(KeyCode::Right, KeyModifiers::CONTROL).unwrap(),
                    key(KeyCode::PageUp, KeyModifiers::NONE).unwrap(),
                    key(KeyCode::PageDown, KeyModifiers::NONE).unwrap(),
                    key(KeyCode::End, KeyModifiers::NONE).unwrap(),
                ]
            );
            assert!(decode(b"\x1b[999~\x1b]dyes\r\x07\x1bPdyes\r\x1b\\")
                .unwrap()
                .is_empty());
            assert!(decode(&[0xf0, 0x28, 0x8c, 0xbc]).is_err());
        }

        #[test]
        fn only_lone_escape_can_time_out_and_body_never_becomes_shortcuts() {
            let mut decoder = Decoder::default();
            assert!(decoder.push(0x1b).unwrap().is_none());
            assert_eq!(
                decoder.finish_escape(),
                key(KeyCode::Esc, KeyModifiers::NONE)
            );
            for &byte in b"\x1b[200~\rdyes\r" {
                assert!(decoder.push(byte).unwrap().is_none());
                if !decoder.is_lone_escape() {
                    assert!(decoder.finish_escape().is_none());
                }
            }
        }

        #[test]
        fn console_unicode_keeps_surrogate_pairs_across_read_boundaries() {
            let mut unicode = Utf16Decoder::default();
            assert_eq!(unicode.push(0x4e2d).unwrap(), Some('中'));
            assert_eq!(unicode.push(0xd83d).unwrap(), None);
            assert_eq!(unicode.push(0xde42).unwrap(), Some('🙂'));
            assert!(unicode.push(0xde42).is_err());
            assert_eq!(unicode.push(0xd83d).unwrap(), None);
            assert!(unicode.push(b'd' as u16).is_err());
        }

        #[test]
        fn control_c_cancels_incomplete_controls_without_replaying_their_contents() {
            for prefix in [
                b"\x1b]q\x1b".as_slice(),
                b"\x1bPq\x1b",
                b"\x1b_q\x1b",
                b"\x1b^q\x1b",
                b"\x1bXq\x1b",
                b"\x1b[200",
                b"\x1bO",
                b"\x1b",
                &[0xf0, 0x9f],
            ] {
                let mut decoder = Decoder::default();
                for &byte in prefix {
                    assert!(decoder.push(byte).unwrap().is_none());
                }
                assert!(decoder
                    .push(3)
                    .unwrap_err()
                    .contains("cancelled an incomplete"));
            }
            assert_eq!(
                decode(b"\x03").unwrap(),
                vec![key(KeyCode::Char('c'), KeyModifiers::CONTROL).unwrap()]
            );
            assert_eq!(
                decode(b"\x1b[200~q\x1b]\x03dyes\r\x1b[201~").unwrap(),
                vec![Event::Paste("q\x1b]\x03dyes\r".to_string())]
            );
        }
    }
}
