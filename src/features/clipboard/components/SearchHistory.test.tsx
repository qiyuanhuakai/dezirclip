import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { SearchHistory } from "./SearchHistory";

describe("SearchHistory", () => {
  it("test_renders_history: 3 history items → renders 3 buttons", () => {
    const items = ["hello world", "test query", "foo bar"];
    const { container } = render(<SearchHistory items={items} onSelect={vi.fn()} />);
    const buttons = container.querySelectorAll("[data-test-history-item]");
    expect(buttons).toHaveLength(3);
    expect(buttons[0]).toHaveTextContent("hello world");
    expect(buttons[1]).toHaveTextContent("test query");
    expect(buttons[2]).toHaveTextContent("foo bar");
  });

  it("test_click_item_calls_on_select: click → onSelect called with item text", () => {
    const onSelect = vi.fn();
    const items = ["first", "second", "third"];
    const { container } = render(<SearchHistory items={items} onSelect={onSelect} />);
    const buttons = container.querySelectorAll("[data-test-history-item]");
    fireEvent.click(buttons[1]);
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith("second");
  });

  it("test_empty_history: empty history → shows empty message", () => {
    const { container } = render(<SearchHistory items={[]} onSelect={vi.fn()} />);
    const empty = container.querySelector("[data-test-history-empty]");
    expect(empty).toHaveTextContent("暂无搜索记录");
  });
});
