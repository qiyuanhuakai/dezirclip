import { canonicalShortcutKey, isSuperShortcutModifier, shortcutKeyFromEvent } from "../lib/shortcutKeys";

interface MatchHotkeyOptions {
  ignoreWin?: boolean;
}

export const matchesHotkey = (
  event: KeyboardEvent,
  hotkeyStr: string,
  opts: MatchHotkeyOptions = {}
) => {
  if (!hotkeyStr) return false;

  const parts = hotkeyStr.split("+").map((p) => p.trim()).filter(Boolean);
  if (parts.length === 0) return false;

  const isModifier = (p: string) => {
    const up = p.toUpperCase();
    return (
      up === "CTRL" ||
      up === "CONTROL" ||
      up === "SHIFT" ||
      up === "ALT" ||
      up === "MENU" ||
      isSuperShortcutModifier(up)
    );
  };

  const ctrl = parts.some((p) => ["CTRL", "CONTROL"].includes(p.toUpperCase()));
  const shift = parts.some((p) => p.toUpperCase() === "SHIFT");
  const alt = parts.some((p) => ["ALT", "MENU"].includes(p.toUpperCase()));
  const win = parts.some(isSuperShortcutModifier);

  if (event.ctrlKey !== ctrl) return false;
  if (event.shiftKey !== shift) return false;
  if (event.altKey !== alt) return false;
  if (!opts.ignoreWin) {
    if (event.metaKey !== win) return false;
  } else {
    if (!win && event.metaKey) return false;
  }

  const keyPart = parts.filter((p) => !isModifier(p)).pop();
  if (!keyPart) return false;

  const expectedKey = canonicalShortcutKey(keyPart);
  return expectedKey !== undefined && shortcutKeyFromEvent(event) === expectedKey;
};
