import { describe, expect, it } from "vitest";
import { normalizePreviewOpacity, readPreviewPreferences } from "./previewPreferences";

describe("preview preferences", () => {
  it("keeps keyboard previews opt-in and defaults opacity to 65", () => {
    expect(readPreviewPreferences({})).toEqual({ keyboardEnabled: false, opacity: 65 });
  });
  it("inherits the current surface opacity until a preview value is saved", () => {
    expect(readPreviewPreferences({ "app.surface_opacity": "42" }).opacity).toBe(42);
    expect(readPreviewPreferences({ "app.surface_opacity": "42", "app.preview_opacity": "73" }).opacity).toBe(73);
  });
  it("only an explicit true enables keyboard previews", () => {
    for (const value of ["false", "1", "TRUE", ""]) expect(readPreviewPreferences({ "app.preview_keyboard_enabled": value }).keyboardEnabled).toBe(false);
    expect(readPreviewPreferences({ "app.preview_keyboard_enabled": "true" }).keyboardEnabled).toBe(true);
  });
  it("keeps zero and full opacity as explicit values", () => {
    expect(readPreviewPreferences({ "app.preview_opacity": "0" }).opacity).toBe(0);
    expect(readPreviewPreferences({ "app.preview_opacity": "100" }).opacity).toBe(100);
  });
  it("rejects malformed numbers and empty values without losing the inherited value", () => {
    for (const value of ["", " ", "no", "NaN", "Infinity", "42px"]) {
      expect(readPreviewPreferences({ "app.surface_opacity": "51", "app.preview_opacity": value }).opacity).toBe(51);
    }
    expect(readPreviewPreferences({ "app.surface_opacity": "bad" }).opacity).toBe(65);
  });
  it("normalizes the slider range to whole percentages", () => {
    expect(normalizePreviewOpacity(-1)).toBe(0);
    expect(normalizePreviewOpacity(101)).toBe(100);
    expect(normalizePreviewOpacity(38.7)).toBe(39);
    expect(normalizePreviewOpacity(Number.NaN)).toBe(65);
  });
});
