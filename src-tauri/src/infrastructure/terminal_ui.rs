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
        };
        crossterm::terminal::enable_raw_mode()
            .map_err(|error| format!("tui cannot enable terminal input: {error}"))?;
        terminal.raw = true;
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

    pub fn read(&self) -> Result<crossterm::event::Event, String> {
        crossterm::event::read().map_err(|error| format!("tui terminal input failed: {error}"))
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
