const PUNCTUATION: Record<string, string> = {
  Backquote: "`", Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\",
  Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/",
};

const names = [
  "Space", "Enter", "Tab", "Backspace", "Delete", "Insert", "Escape", "PageUp", "PageDown", "Home", "End",
  "Left", "Right", "Up", "Down", "CapsLock", "NumLock", "ScrollLock", "PrintScreen", "Pause", "ContextMenu",
  "NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide", "NumpadDecimal", "NumpadEnter", "NumpadEqual",
  "VolumeMute", "VolumeDown", "VolumeUp", "MediaTrackNext", "MediaTrackPrevious", "MediaStop", "MediaPlayPause",
  "MouseMiddle", ...Object.keys(PUNCTUATION),
];
const namesAndAliases = new Map(names.map(name => [name.toUpperCase(), name]));
for (const [name, symbol] of Object.entries(PUNCTUATION)) namesAndAliases.set(symbol, name);
for (const [alias, name] of Object.entries({
  SPACEBAR: "Space", RETURN: "Enter", DEL: "Delete", INS: "Insert", ESC: "Escape",
  ARROWLEFT: "Left", ARROWRIGHT: "Right", ARROWUP: "Up", ARROWDOWN: "Down",
  PLUS: "Equal", NumpadPlus: "NumpadAdd", NumPlus: "NumpadAdd",
  NumAdd: "NumpadAdd", NumSub: "NumpadSubtract", NumSubtract: "NumpadSubtract",
  NumMult: "NumpadMultiply", NumMultiply: "NumpadMultiply", NumDiv: "NumpadDivide", NumDivide: "NumpadDivide",
  NumDec: "NumpadDecimal", NumDecimal: "NumpadDecimal", NumEnter: "NumpadEnter", NumEqual: "NumpadEqual",
  AudioVolumeMute: "VolumeMute", AudioVolumeDown: "VolumeDown", AudioVolumeUp: "VolumeUp",
  MediaNextTrack: "MediaTrackNext", MediaPreviousTrack: "MediaTrackPrevious", MediaTrackPrev: "MediaTrackPrevious",
  PauseBreak: "Pause", Apps: "ContextMenu", MButton: "MouseMiddle",
})) namesAndAliases.set(alias.toUpperCase(), name);

export function canonicalShortcutKey(key: string): string | undefined {
  const upper = key.toUpperCase();
  if (/^[A-Z0-9]$|^F(?:[1-9]|1[0-9]|2[0-4])$/.test(upper)) return upper;
  if (/^NUM(?:PAD)?[0-9]$/.test(upper)) return "Numpad" + upper.slice(-1);
  if (/^KEY[A-Z]$|^DIGIT[0-9]$/.test(upper)) return upper.slice(-1);
  return namesAndAliases.get(upper);
}

export function shortcutKeyFromEvent({ key, code = "" }: { key: string; code?: string }): string | undefined {
  if (code.startsWith("Numpad") || Object.prototype.hasOwnProperty.call(PUNCTUATION, code) || /^Digit[0-9]$/.test(code)) {
    return canonicalShortcutKey(code);
  }
  return key === " " ? "Space" : canonicalShortcutKey(key) ?? canonicalShortcutKey(code);
}

export const isSuperShortcutModifier = (key: string) => /^(win|windows|super|meta|cmd|command)$/i.test(key.trim());

export function normalizeShortcutForCompare(value: string): string {
  const parts = value.split("+").map(part => part.trim().toUpperCase());
  const key = parts.pop() ?? "";
  const modifiers = parts.map(part => {
    if (isSuperShortcutModifier(part)) return "SUPER";
    if (part === "CONTROL") return "CTRL";
    if (part === "MENU") return "ALT";
    return part;
  });
  return modifiers.sort().concat(canonicalShortcutKey(key) ?? key).join("+");
}
