use super::*;
use ::windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL,
            MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
        },
        WindowsAndMessaging::{
            GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY,
        },
    },
};
const STOP: u32 = WM_APP + 0x392;

pub(super) fn run(
    chord: Chord,
    control: Arc<Control>,
    ready: mpsc::SyncSender<PlatformResult<()>>,
    callback: &mut dyn FnMut(),
) -> PlatformResult<()> {
    unsafe {
        let thread = GetCurrentThreadId();
        let mut message = MSG::default();
        // Materialize the thread queue before publishing a cancellation target.
        PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
        control.install(Box::new(move || {
            PostThreadMessageW(thread, STOP, WPARAM(0), LPARAM(0)).map_err(|_| failed())
        }))?;
        if control.cancelled() {
            return Ok(());
        }
        let mut modifiers: HOT_KEY_MODIFIERS = MOD_NOREPEAT;
        for (mask, native) in [
            (CTRL, MOD_CONTROL),
            (ALT, MOD_ALT),
            (SHIFT, MOD_SHIFT),
            (SUPER, MOD_WIN),
        ] {
            if chord.modifiers & mask != 0 {
                modifiers |= native;
            }
        }
        // F12 is reserved for the debugger by the native API contract.
        if chord.key.windows == 0x7b {
            return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
        }
        RegisterHotKey(None, 1, modifiers, chord.key.windows)
            .map_err(|_| unsupported(UnsupportedReason::PermissionDenied))?;
        control.running.store(true, Ordering::Release);
        let result = (|| {
            if control.cancelled() {
                return Ok(());
            }
            ready.send(Ok(())).map_err(|_| failed())?;
            loop {
                let status = GetMessageW(&mut message, None, 0, 0).0;
                if status == -1 {
                    return Err(failed());
                }
                if status == 0 || message.message == STOP || control.cancelled() {
                    return Ok(());
                }
                if message.message == WM_HOTKEY && message.wParam.0 == 1 {
                    control.pressed(callback);
                }
            }
        })();
        control.clear();
        let released = UnregisterHotKey(None, 1).map_err(|_| failed());
        result.and(released)
    }
}
