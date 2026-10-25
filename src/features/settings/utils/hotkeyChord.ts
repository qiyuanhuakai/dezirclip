export function hotkeyChord(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">): string | null {
  const key = event.key === " " ? "Space" : event.key.toUpperCase();
  if (["CONTROL", "SHIFT", "ALT", "META"].includes(key)) return null;
  const modifiers = [];
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.altKey) modifiers.push("Alt");
  if (event.metaKey) modifiers.push("Win");
  return [...modifiers, key].join("+");
}
