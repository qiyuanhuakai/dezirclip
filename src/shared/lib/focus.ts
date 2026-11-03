import { invoke } from "./desktopInvoke";

export async function focusClipboardWindow(): Promise<void> {
  await invoke("focus_clipboard_window");
}

export async function restoreLastFocus(): Promise<void> {
  await invoke("restore_last_focus");
}
