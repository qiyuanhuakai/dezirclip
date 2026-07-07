import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useThemeSync } from "./useThemeSync";

vi.mock("../lib/themeRuntime", () => ({
  applyThemeClass: vi.fn(),
  applyModeClass: vi.fn(),
}));

import { applyThemeClass, applyModeClass } from "../lib/themeRuntime";
const mockApplyThemeClass = vi.mocked(applyThemeClass);
const mockApplyModeClass = vi.mocked(applyModeClass);

describe("useThemeSync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("applies theme and mode from storage on mount", () => {
    localStorage.setItem("dezirclip_theme", "retro");
    localStorage.setItem("dezirclip_color_mode", "dark");
    renderHook(() => useThemeSync());
    expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "retro"
    );
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "dark"
    );
  });

  it("falls back to tiez prefixed keys when dezirclip keys are absent", () => {
    localStorage.setItem("tiez_theme", "scifi");
    localStorage.setItem("tiez_color_mode", "light");
    renderHook(() => useThemeSync());
    expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "scifi"
    );
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "light"
    );
  });

  it("dispatches dezirclip:theme-changed event and hook re-applies theme", () => {
    localStorage.setItem("dezirclip_theme", "mica");
    localStorage.setItem("dezirclip_color_mode", "light");
    renderHook(() => useThemeSync());
    vi.clearAllMocks();

    act(() => {
      window.dispatchEvent(
        new CustomEvent("dezirclip:theme-changed", {
          detail: { theme: "scifi", colorMode: "dark" },
        })
      );
    });

    expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "scifi"
    );
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "dark"
    );
  });

  it("applies defaults when no storage keys exist", () => {
    renderHook(() => useThemeSync());
    expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "mica"
    );
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "light"
    );
  });
});
