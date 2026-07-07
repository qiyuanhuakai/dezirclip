import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useThemeSync } from "./useThemeSync";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("../lib/themeRuntime", () => ({
  applyThemeClass: vi.fn(),
  applyModeClass: vi.fn(),
  ensureThemeCssLoaded: vi.fn(() => Promise.resolve()),
}));

import { applyThemeClass, applyModeClass } from "../lib/themeRuntime";
import { listen } from "@tauri-apps/api/event";
const mockApplyThemeClass = vi.mocked(applyThemeClass);
const mockApplyModeClass = vi.mocked(applyModeClass);
const mockListen = vi.mocked(listen);

describe("useThemeSync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    mockListen.mockResolvedValue(vi.fn());
  });

  it("applies theme and mode from storage on mount", async () => {
    localStorage.setItem("dezirclip_theme", "retro");
    localStorage.setItem("dezirclip_color_mode", "dark");
    renderHook(() => useThemeSync());
    await waitFor(() => expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "retro"
    ));
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "dark"
    );
  });

  it("falls back to tiez prefixed keys when dezirclip keys are absent", async () => {
    localStorage.setItem("tiez_theme", "scifi");
    localStorage.setItem("tiez_color_mode", "light");
    renderHook(() => useThemeSync());
    await waitFor(() => expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "scifi"
    ));
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "light"
    );
  });

  it("dispatches dezirclip:theme-changed event and hook re-applies theme", async () => {
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

    await waitFor(() => expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "scifi"
    ));
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "dark"
    );
  });

  it("applies Tauri theme-changed broadcasts for sibling windows", async () => {
    let handler: ((event: { payload: { theme: string; color_mode: "light" | "dark" } }) => void) | null = null;
    mockListen.mockImplementation(async (_event, callback) => {
      handler = callback as typeof handler;
      return () => undefined;
    });

    renderHook(() => useThemeSync());
    vi.clearAllMocks();

    act(() => {
      handler?.({ payload: { theme: "scifi", color_mode: "light" } });
    });

    await waitFor(() => expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "scifi"
    ));
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "light"
    );
  });

  it("applies defaults when no storage keys exist", async () => {
    renderHook(() => useThemeSync());
    await waitFor(() => expect(mockApplyThemeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "mica"
    ));
    expect(mockApplyModeClass).toHaveBeenCalledWith(
      document.documentElement,
      document.body,
      "light"
    );
  });
});
