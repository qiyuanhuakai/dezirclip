//! Explicit terminal clipboard writes using xterm OSC 52.

use base64::Engine;
use std::io::{IsTerminal, Write};

/// The limit applies to original UTF-8 bytes, before base64 expansion.
pub const MAX_TEXT_BYTES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passthrough {
    None,
    Tmux,
}

/// Encode one clipboard write, without querying or reading the terminal.
pub fn encode(text: &str, passthrough: Passthrough) -> Result<Vec<u8>, String> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "OSC 52 text exceeds the {MAX_TEXT_BYTES}-byte UTF-8 limit ({} bytes); nothing was sent",
            text.len()
        ));
    }
    let payload = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let sequence = format!("\x1b]52;c;{payload}\x1b\\");
    match passthrough {
        Passthrough::None => Ok(sequence.into_bytes()),
        Passthrough::Tmux => {
            let mut wrapped = Vec::with_capacity(sequence.len() + 12);
            wrapped.extend_from_slice(b"\x1bPtmux;");
            for byte in sequence.bytes() {
                wrapped.push(byte);
                if byte == 0x1b {
                    wrapped.push(byte);
                }
            }
            wrapped.extend_from_slice(b"\x1b\\");
            Ok(wrapped)
        }
    }
}

/// Success means bytes were written; terminal permission and support determine
/// whether the clipboard changes. There is deliberately no readback request.
pub fn write_text(text: &str, passthrough: Passthrough) -> Result<(), String> {
    let sequence = encode(text, passthrough)?;
    #[cfg(target_os = "linux")]
    let device = "/dev/tty";
    #[cfg(target_os = "windows")]
    let device = "CONOUT$";
    let mut terminal = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(device)
        .map_err(|error| format!(
            "OSC 52 requires a controlling terminal ({device}): {error}; run inside an interactive terminal or an SSH session with a PTY"
        ))?;
    if !terminal.is_terminal() {
        return Err("OSC 52 requires a controlling terminal; nothing was sent".to_string());
    }
    #[cfg(target_os = "windows")]
    let _mode = ConsoleMode::enable(&terminal)?;
    terminal
        .write_all(&sequence)
        .and_then(|()| terminal.flush())
        .map_err(|error| format!("OSC 52 terminal write failed: {error}"))
}

#[cfg(target_os = "windows")]
struct ConsoleMode {
    handle: windows::Win32::Foundation::HANDLE,
    previous: windows::Win32::System::Console::CONSOLE_MODE,
}

#[cfg(target_os = "windows")]
impl ConsoleMode {
    fn enable(terminal: &std::fs::File) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Console::{
            GetConsoleMode, SetConsoleMode, CONSOLE_MODE, ENABLE_PROCESSED_OUTPUT,
            ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        };
        let handle = HANDLE(terminal.as_raw_handle());
        let mut previous = CONSOLE_MODE::default();
        unsafe {
            GetConsoleMode(handle, &mut previous)
                .map_err(|error| format!("OSC 52 cannot inspect console mode: {error}"))?;
            SetConsoleMode(
                handle,
                previous | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
            )
            .map_err(|error| {
                format!("OSC 52 requires a console with virtual terminal support: {error}")
            })?;
        }
        Ok(Self { handle, previous })
    }
}

#[cfg(target_os = "windows")]
impl Drop for ConsoleMode {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Console::SetConsoleMode(self.handle, self.previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_uses_clipboard_selection_and_string_terminator() {
        assert_eq!(
            encode("hello", Passthrough::None).unwrap(),
            b"\x1b]52;c;aGVsbG8=\x1b\\"
        );
        assert_eq!(encode("", Passthrough::None).unwrap(), b"\x1b]52;c;\x1b\\");
    }

    #[test]
    fn base64_preserves_utf8_whitespace_newlines_and_control_bytes() {
        let text = "  \u{4f60}\u{597d}\r\n\tline \n\x1b\x07\0";
        let sequence = encode(text, Passthrough::None).unwrap();
        let payload = &sequence[b"\x1b]52;c;".len()..sequence.len() - 2];
        assert!(!payload.contains(&b'\n'));
        assert!(!payload.contains(&0x1b));
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(payload)
                .unwrap(),
            text.as_bytes()
        );
    }

    #[test]
    fn tmux_wraps_one_layer_and_doubles_each_inner_escape() {
        assert_eq!(
            encode("hi", Passthrough::Tmux).unwrap(),
            b"\x1bPtmux;\x1b\x1b]52;c;aGk=\x1b\x1b\\\x1b\\"
        );
    }

    #[test]
    fn byte_limit_accepts_boundary_and_rejects_oversize_without_truncation() {
        let boundary = "x".repeat(MAX_TEXT_BYTES);
        let sequence = encode(&boundary, Passthrough::None).unwrap();
        let payload = &sequence[7..sequence.len() - 2];
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(payload)
                .unwrap()
                .len(),
            MAX_TEXT_BYTES
        );
        assert!(encode(&(boundary + "x"), Passthrough::None).is_err());
        assert!(encode(
            &"\u{4f60}".repeat(MAX_TEXT_BYTES / 3 + 1),
            Passthrough::Tmux
        )
        .is_err());
    }
}
