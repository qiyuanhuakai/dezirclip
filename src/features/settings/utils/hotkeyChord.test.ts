import { describe, expect, it } from "vitest";
import { hotkeyChord } from "./hotkeyChord";

const event = (key: string, modifiers: Partial<KeyboardEvent> = {}) => ({ key, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...modifiers });

describe("settings hotkey serialization", () => {
  it("preserves a named Space key rather than a trimmed empty token", () => {
    expect(hotkeyChord(event(" ", { ctrlKey: true, altKey: true }))).toBe("Ctrl+Alt+Space");
  });
  it("ignores modifier-only key presses", () => {
    for (const key of ["Control", "Shift", "Alt", "Meta"]) expect(hotkeyChord(event(key))).toBeNull();
  });
  it("uses the same deterministic modifier order as global shortcuts", () => {
    expect(hotkeyChord(event("p", { ctrlKey: true, shiftKey: true, altKey: true, metaKey: true }))).toBe("Ctrl+Shift+Alt+Win+P");
  });
  it("retains named navigation and function keys", () => {
    for (const [key, expected] of [["ArrowLeft", "ARROWLEFT"], ["PageDown", "PAGEDOWN"], ["F11", "F11"], ["Delete", "DELETE"]]) {
      expect(hotkeyChord(event(key, { ctrlKey: true }))).toBe("Ctrl+" + expected);
    }
  });
});
