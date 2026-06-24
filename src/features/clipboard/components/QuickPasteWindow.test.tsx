import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, screen, act } from "@testing-library/react";
import QuickPasteWindow from "./QuickPasteWindow";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

vi.mock("../../../shared/lib/themeRuntime", () => ({
  applyThemeClass: vi.fn(),
  applyModeClass: vi.fn(),
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
