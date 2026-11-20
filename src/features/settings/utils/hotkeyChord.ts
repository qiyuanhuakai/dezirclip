import { shortcutKeyFromEvent } from "../../../shared/lib/shortcutKeys";

type HotkeyEvent = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">
  & Partial<Pick<KeyboardEvent, "code" | "isComposing">>;

export function hotkeyChord(event: HotkeyEvent): string | null {
  if (event.isComposing) return null;
  const key = shortcutKeyFromEvent(event);
  if (!key || key === "MouseMiddle") return null;
  const modifiers = [];
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Super");
  return [...modifiers, key].join("+");
}
