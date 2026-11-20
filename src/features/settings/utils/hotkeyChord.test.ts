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
    expect(hotkeyChord(event("p", { ctrlKey: true, shiftKey: true, altKey: true, metaKey: true }))).toBe("Ctrl+Alt+Shift+Super+P");
  });
  it("retains named navigation and function keys", () => {
    for (const [key, expected] of [["ArrowLeft", "Left"], ["PageDown", "PageDown"], ["F11", "F11"], ["Delete", "Delete"]]) {
      expect(hotkeyChord(event(key, { ctrlKey: true }))).toBe("Ctrl+" + expected);
    }
  });
});

describe("extended keyboard shortcuts", () => {
  it.each([
    ["+", "Equal", "Shift+Equal"], ["!", "Digit1", "Shift+1"],
    [":", "Semicolon", "Shift+Semicolon"],
    ["1", "Numpad1", "Shift+Numpad1"], ["End", "Numpad1", "Shift+Numpad1"],
    ["+", "NumpadAdd", "Shift+NumpadAdd"], [".", "NumpadDecimal", "Shift+NumpadDecimal"],
    ["PrintScreen", "PrintScreen", "Shift+PrintScreen"], ["Pause", "Pause", "Shift+Pause"],
    ["ScrollLock", "ScrollLock", "Shift+ScrollLock"],
    ["AudioVolumeUp", "AudioVolumeUp", "Shift+VolumeUp"], ["Escape", "Escape", "Shift+Escape"],
  ])("records %s from %s as %s", (key, code, expected) => {
    expect(hotkeyChord(event(key, { code, shiftKey: true }))).toBe(expected);
  });

  it("ignores IME composition and unknown keys", () => {
    expect(hotkeyChord(event("Process", { code: "KeyA", isComposing: true }))).toBeNull();
    expect(hotkeyChord(event("Unidentified"))).toBeNull();
    expect(hotkeyChord(event("AltGraph", { code: "AltRight" }))).toBeNull();
  });
});
