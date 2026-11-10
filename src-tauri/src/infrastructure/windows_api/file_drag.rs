use windows::Win32::{
    Foundation::GlobalFree,
    System::{Com::{IDataObject, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL, DVASPECT_CONTENT},
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        Ole::{CF_HDROP, DROPEFFECT_COPY}},
    UI::{Shell::{SHCreateDataObject, SHDoDragDrop}, Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON}},
};

pub fn start(window: &tauri::WebviewWindow, paths: Vec<String>, done: impl FnOnce(Result<(), String>)) {
    done(start_files(window, &paths));
}

fn start_files(window: &tauri::WebviewWindow, paths: &[String]) -> Result<(), String> {
    if !window.is_visible().map_err(|e| e.to_string())? { return Err("WindowUnavailable".into()); }
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    // This is a file copy gesture. No clipboard mutation or synthetic key input.
    unsafe {
        if GetAsyncKeyState(VK_LBUTTON.0 as i32) >= 0 { return Ok(()); }
        let mut names = Vec::<u16>::new();
        for path in paths { names.extend(path.encode_utf16()); names.push(0); }
        names.push(0);
        let data: IDataObject = SHCreateDataObject(None, None, None).map_err(|e| e.to_string())?;
        let allocation = GlobalAlloc(GMEM_MOVEABLE, 20 + names.len() * 2).map_err(|e| e.to_string())?;
        let memory = GlobalLock(allocation).cast::<u8>();
        if memory.is_null() { let _ = GlobalFree(Some(allocation)); return Err("FileDragAllocationFailed".into()); }
        std::ptr::write_bytes(memory, 0, 20);
        std::ptr::write_unaligned(memory.cast::<u32>(), 20);
        std::ptr::write_unaligned(memory.add(16).cast::<u32>(), 1);
        std::ptr::copy_nonoverlapping(names.as_ptr().cast::<u8>(), memory.add(20), names.len() * 2);
        let _ = GlobalUnlock(allocation);
        let format = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
        let medium = STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: allocation }, pUnkForRelease: std::mem::ManuallyDrop::new(None) };
        if let Err(error) = data.SetData(&format, &medium, true) {
            let _ = GlobalFree(Some(allocation));
            return Err(error.to_string());
        }
        SHDoDragDrop(Some(hwnd), &data, None, DROPEFFECT_COPY)
            .map(|_| ()).map_err(|e| e.to_string())
    }
}
