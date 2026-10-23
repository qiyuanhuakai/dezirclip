use crate::infrastructure::paste_input::{self, Encoding, InputDriver};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::AttachThreadInput;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
    KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub fn send_paste(method: &str, content: Option<&str>, content_type: Option<&str>) -> Result<(), String> {
    let mut driver = NativeInput { text_context: None, scan_codes: Vec::new() };
    paste_input::send_paste(&mut driver, method, content, content_type).map_err(|error| error.to_string())
}

struct NativeInput {
    text_context: Option<(HWND, GameModeThreadContext, GameModeImeContext)>,
    scan_codes: Vec<(u16, u16)>,
}
impl NativeInput {
    fn scan(&mut self, key: u16) -> u16 {
        if let Some((_, scan)) = self.scan_codes.iter().find(|(vk, _)| *vk == key) { return *scan; }
        let scan = unsafe { MapVirtualKeyW(key as u32, MAPVK_VK_TO_VSC) as u16 };
        self.scan_codes.push((key, scan));
        scan
    }
}
impl InputDriver for NativeInput {
    fn send(&mut self, inputs: &[paste_input::Input]) -> usize {
        let inputs: Vec<_> = inputs.iter().map(|input| {
            let mut flags = if input.up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
            let (vk, scan) = match input.encoding {
                Encoding::Virtual => (VIRTUAL_KEY(input.key), 0),
                Encoding::Unicode => { flags |= KEYEVENTF_UNICODE; (VIRTUAL_KEY(0), input.key) }
                Encoding::Scan | Encoding::ScanWithVirtual | Encoding::ExtendedScan => {
                    flags |= KEYEVENTF_SCANCODE;
                    if input.encoding == Encoding::ExtendedScan { flags |= KEYEVENTF_EXTENDEDKEY; }
                    let vk = if input.encoding == Encoding::Scan { VIRTUAL_KEY(0) } else { VIRTUAL_KEY(input.key) };
                    (vk, self.scan(input.key))
                }
            };
            INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, ..Default::default() } } }
        }).collect();
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) as usize }
    }
    fn pause(&mut self, millis: u64) { std::thread::sleep(std::time::Duration::from_millis(millis)); }
    fn key_is_down(&self, key: u16) -> bool { unsafe { GetAsyncKeyState(key as i32) < 0 } }
    fn begin_text(&mut self) -> bool {
        unsafe {
            let target = GetForegroundWindow();
            if target.0.is_null() { return false; }
            self.text_context = Some((target, attach_game_mode_thread(target), setup_game_mode_ime(target)));
        }
        true
    }
    fn same_target(&self) -> bool {
        self.text_context.as_ref().is_some_and(|(target, _, _)| unsafe { GetForegroundWindow() == *target })
    }
    fn end_text(&mut self) {
        if let Some((target, thread, ime)) = self.text_context.take() {
            unsafe { restore_game_mode_ime(target, &ime); restore_game_mode_thread(&thread); }
        }
    }
}
impl Drop for NativeInput { fn drop(&mut self) { self.end_text(); } }

struct GameModeThreadContext {
    current_thread: u32,
    target_thread: u32,
    attached: bool,
}

struct GameModeImeContext {
    has_himc: bool,
    himc: windows::Win32::UI::Input::Ime::HIMC,
    ime_open: bool,
    ime_conv: windows::Win32::UI::Input::Ime::IME_CONVERSION_MODE,
    ime_sentence: windows::Win32::UI::Input::Ime::IME_SENTENCE_MODE,
}

unsafe fn attach_game_mode_thread(target_hwnd: HWND) -> GameModeThreadContext {
    let target_thread = GetWindowThreadProcessId(target_hwnd, None);
    let current_thread = windows::Win32::System::Threading::GetCurrentThreadId();
    let mut attached = false;
    if target_thread != 0 && target_thread != current_thread {
        if AttachThreadInput(current_thread, target_thread, true).as_bool() {
            attached = true;
        }
    }
    GameModeThreadContext {
        current_thread,
        target_thread,
        attached,
    }
}

unsafe fn restore_game_mode_thread(ctx: &GameModeThreadContext) {
    if ctx.attached {
        let _ = AttachThreadInput(ctx.current_thread, ctx.target_thread, false);
    }
}

unsafe fn setup_game_mode_ime(target_hwnd: HWND) -> GameModeImeContext {
    use windows::Win32::UI::Input::Ime::{
        ImmGetContext, ImmGetConversionStatus, ImmGetOpenStatus, ImmSetConversionStatus,
        ImmSetOpenStatus, IME_CMODE_ALPHANUMERIC, IME_CONVERSION_MODE, IME_SENTENCE_MODE,
        IME_SMODE_NONE,
    };
    let himc = ImmGetContext(target_hwnd);
    let mut ime_open = false;
    let mut ime_conv = IME_CONVERSION_MODE(0);
    let mut ime_sentence = IME_SENTENCE_MODE(0);
    let mut has_himc = false;
    if !himc.0.is_null() {
        has_himc = true;
        ime_open = ImmGetOpenStatus(himc).as_bool();
        let _ = ImmGetConversionStatus(himc, Some(&mut ime_conv), Some(&mut ime_sentence));
        if ime_open {
            let _ = ImmSetOpenStatus(himc, false);
        }
        let _ = ImmSetConversionStatus(himc, IME_CMODE_ALPHANUMERIC, IME_SMODE_NONE);
    }
    GameModeImeContext {
        has_himc,
        himc,
        ime_open,
        ime_conv,
        ime_sentence,
    }
}

unsafe fn restore_game_mode_ime(target_hwnd: HWND, ctx: &GameModeImeContext) {
    use windows::Win32::UI::Input::Ime::{
        ImmReleaseContext, ImmSetConversionStatus, ImmSetOpenStatus,
    };
    if ctx.has_himc {
        let _ = ImmSetConversionStatus(ctx.himc, ctx.ime_conv, ctx.ime_sentence);
        if ctx.ime_open {
            let _ = ImmSetOpenStatus(ctx.himc, true);
        }
        let _ = ImmReleaseContext(target_hwnd, ctx.himc);
    }
}
