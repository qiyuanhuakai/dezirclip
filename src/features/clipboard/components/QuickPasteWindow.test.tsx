import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, screen, act } from "@testing-library/react";
import QuickPasteWindow from "./QuickPasteWindow";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

vi.mock("../../../shared/lib/themeRuntime", () => ({
  applyThemeClass: vi.fn(),
  applyModeClass: vi.fn(),
  ensureThemeCssLoaded: vi.fn(() => Promise.resolve()),
}));

function makeEntry(id: number, content: string) {
  return {
    id,
    content_type: "text" as const,
    content,
    source_app: "test-app",
    timestamp: Date.now(),
    preview: content,
    is_pinned: false,
    tags: [],
  };
}

const ENTRIES = Array.from({ length: 10 }, (_, i) =>
  makeEntry(i + 1, `entry ${i + 1}`)
);

describe("QuickPasteWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_clipboard_history") return ENTRIES;
      return null;
    });
    Storage.prototype.getItem = vi.fn(() => null);
  });

  it("renders entries", async () => {
    await act(async () => {
      render(<QuickPasteWindow />);
    });

    const items = screen.getAllByTestId("quick-paste-item");
    expect(items).toHaveLength(10);
    expect(items[0]).toHaveTextContent("entry 1");
    expect(items[9]).toHaveTextContent("entry 10");
    expect(mockInvoke).toHaveBeenCalledWith("get_clipboard_history", {
      limit: 10,
      offset: 0,
      contentType: null,
    });
  });

  it("keyboard nav arrow down increments activeIndex", async () => {
    await act(async () => {
      render(<QuickPasteWindow />);
    });

    const items = screen.getAllByTestId("quick-paste-item");
    expect(items[0]).toHaveClass("quick-paste-window__item--active");

    await act(async () => {
      fireEvent.keyDown(window, { key: "ArrowDown" });
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "ArrowDown" });
    });

    expect(items[2]).toHaveClass("quick-paste-window__item--active");
  });

  it("keyboard nav enter pastes and hides", async () => {
    await act(async () => {
      render(<QuickPasteWindow />);
    });

    await act(async () => {
      fireEvent.keyDown(window, { key: "Enter" });
    });

    expect(mockInvoke).toHaveBeenCalledWith("paste_quick_paste_selection", {
      entryId: 1,
    });
    expect(mockInvoke).toHaveBeenCalledWith("hide_quick_paste");
  });

  it("keyboard nav escape hides", async () => {
    await act(async () => {
      render(<QuickPasteWindow />);
    });

    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });

    expect(mockInvoke).toHaveBeenCalledWith("hide_quick_paste");
  });

  it("empty state shows placeholder", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_clipboard_history") return [];
      return null;
    });

    await act(async () => {
      render(<QuickPasteWindow />);
    });

    expect(screen.getByTestId("quick-paste-empty")).toBeInTheDocument();
    expect(screen.getByTestId("quick-paste-empty")).toHaveTextContent(
      "暂无最近记录"
    );
  });
});

describe("QuickPasteWindow — liquid-glass theme integration", () => {
  let styleEl: HTMLStyleElement;

  beforeAll(() => {
    // The liquid-glass.css panel override under test.
    // vitest does not process CSS imports, so inject the rule explicitly.
    styleEl = document.createElement("style");
    styleEl.textContent =
      "body.theme-liquid-glass.quick-paste," +
      "body.theme-liquid-glass.compact-preview," +
      "body.theme-liquid-glass.region-select {" +
        "--surface-dialog-radius: 14px;" +
      "}";
    document.head.appendChild(styleEl);
  });

  afterAll(() => {
    styleEl.remove();
  });

  beforeEach(() => {
    document.body.classList.remove(
      "theme-liquid-glass",
      "quick-paste",
      "compact-preview",
      "region-select"
    );
  });

  it("--surface-dialog-radius resolves to 14px when body has theme-liquid-glass + quick-paste", async () => {
    // Simulate what applyThemeClass("liquid-glass") does in production: add the theme
    // class to body. QuickPasteWindow.tsx then adds `quick-paste` via its useEffect.
    document.body.classList.add("theme-liquid-glass");

    await act(async () => {
      render(<QuickPasteWindow />);
    });

    // The liquid-glass rule targets body, so the cascaded value lives on body.
    // (documentElement keeps the global 16px from :root -- CSS custom properties
    // inherit parent -> child, never child -> parent.)
    const dialogRadius = getComputedStyle(document.body)
      .getPropertyValue("--surface-dialog-radius")
      .trim();
    expect(dialogRadius).toBe("14px");
  });
});
