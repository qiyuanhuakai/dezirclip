import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, fireEvent, screen, act } from "@testing-library/react";
import { SearchBar } from "./SearchBar";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

function makeEntry(id: number, content: string) {
  return {
    id,
    content_type: "text",
    content,
    source_app: "test",
    timestamp: Date.now(),
    preview: content,
    is_pinned: false,
    tags: [],
  };
}

describe("SearchBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "search_fts") return [makeEntry(1, "hello world")];
      if (cmd === "search_fuzzy") return [makeEntry(2, "fuzzy match")];
      if (cmd === "search_regex") return [makeEntry(3, "regex match")];
      if (cmd === "get_search_history") return ["prev search", "older query"];
      return [];
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("test_renders_input: input element exists", () => {
    render(<SearchBar />);
    const input = screen.getByPlaceholderText("搜索剪贴板内容...");
    expect(input).toBeInTheDocument();
    expect(input.tagName).toBe("INPUT");
  });

  it("test_mode_indicator_default_fts: FTS5 mode is default", () => {
    render(<SearchBar />);
    const radios = screen.getAllByRole("radio");
    expect(radios[0]).toHaveAttribute("aria-checked", "true");
    expect(radios[0]).toHaveTextContent("FTS5");
  });

  it("test_mode_switch_to_fuzzy: click 模糊 → mode changes", () => {
    render(<SearchBar />);
    const radios = screen.getAllByRole("radio");
    expect(radios[0]).toHaveAttribute("aria-checked", "true");

    fireEvent.click(radios[1]);

    expect(radios[1]).toHaveAttribute("aria-checked", "true");
    expect(radios[1]).toHaveTextContent("模糊");
    expect(radios[0]).toHaveAttribute("aria-checked", "false");
  });

  it("test_clear_button: type query → click clear → query empty", () => {
    render(<SearchBar />);
    const input = screen.getByPlaceholderText("搜索剪贴板内容...") as HTMLInputElement;

    fireEvent.change(input, { target: { value: "test query" } });
    expect(input.value).toBe("test query");

    const clearBtn = screen.getByRole("button", { name: "清除" });
    fireEvent.click(clearBtn);

    expect(input.value).toBe("");
  });

  it("test_recent_dropdown: history returned from backend → dropdown shows", async () => {
    const { container } = render(<SearchBar />);
    const input = screen.getByPlaceholderText("搜索剪贴板内容...");

    await act(async () => {
      fireEvent.focus(input);
    });

    const history = container.querySelector("[data-test-search-history]");
    expect(history).toBeInTheDocument();

    const items = container.querySelectorAll("[data-test-history-item]");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("prev search");
    expect(items[1]).toHaveTextContent("older query");
  });

  it("debounced search triggers invoke after delay", async () => {
    render(<SearchBar />);
    const input = screen.getByPlaceholderText("搜索剪贴板内容...");

    fireEvent.change(input, { target: { value: "hello" } });

    expect(mockInvoke).not.toHaveBeenCalledWith(
      "search_fts",
      expect.anything()
    );

    await act(async () => {
      vi.advanceTimersByTime(300);
    });

    expect(mockInvoke).toHaveBeenCalledWith("search_fts", {
      query: "hello",
      limit: 50,
    });
  });

  it("stale_state: rapid mode switches → only latest mode search runs", async () => {
    render(<SearchBar />);
    const input = screen.getByPlaceholderText("搜索剪贴板内容...");
    const radios = screen.getAllByRole("radio");

    fireEvent.change(input, { target: { value: "test" } });

    fireEvent.click(radios[1]);
    fireEvent.click(radios[2]);

    await act(async () => {
      vi.advanceTimersByTime(300);
    });

    expect(mockInvoke).toHaveBeenCalledWith("search_regex", {
      pattern: "test",
      limit: 50,
    });
    expect(mockInvoke).not.toHaveBeenCalledWith(
      "search_fuzzy",
      expect.anything()
    );
  });

  it("cancel_resume: type → clear → type again → debounce resets", async () => {
    render(<SearchBar />);
    const input = screen.getByPlaceholderText("搜索剪贴板内容...") as HTMLInputElement;

    fireEvent.change(input, { target: { value: "first" } });
    vi.advanceTimersByTime(150);

    fireEvent.change(input, { target: { value: "" } });
    vi.advanceTimersByTime(150);

    expect(mockInvoke).not.toHaveBeenCalledWith(
      "search_fts",
      expect.anything()
    );

    fireEvent.change(input, { target: { value: "second" } });

    await act(async () => {
      vi.advanceTimersByTime(300);
    });

    expect(mockInvoke).toHaveBeenCalledWith("search_fts", {
      query: "second",
      limit: 50,
    });
  });
});
