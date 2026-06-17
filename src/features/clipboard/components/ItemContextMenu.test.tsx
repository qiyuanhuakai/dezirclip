import { describe, it, expect, vi } from "vitest";
import { render, fireEvent, screen } from "@testing-library/react";
import { ItemContextMenu } from "./ItemContextMenu";

describe("ItemContextMenu", () => {
  it("renders four placeholder items", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <ItemContextMenu x={100} y={100} onSelect={onSelect} onClose={onClose} />
    );

    const items = screen.getAllByRole("menuitem");
    expect(items).toHaveLength(4);
    expect(items[0]).toHaveTextContent("复制");
    expect(items[1]).toHaveTextContent("编辑标签");
    expect(items[2]).toHaveTextContent("文本转换 →");
    expect(items[3]).toHaveTextContent("删除");
  });

  it("keyboard nav ArrowDown increments activeIndex", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <ItemContextMenu x={100} y={100} onSelect={onSelect} onClose={onClose} />
    );

    const menu = screen.getByRole("menu");
    fireEvent.keyDown(menu, { key: "ArrowDown" });

    const items = screen.getAllByRole("menuitem");
    expect(items[0].style.background).toBe("transparent");
    expect(items[1].style.background).not.toBe("transparent");
  });

  it("keyboard nav Enter calls onSelect with active index", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <ItemContextMenu x={100} y={100} onSelect={onSelect} onClose={onClose} />
    );

    const menu = screen.getByRole("menu");
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    fireEvent.keyDown(menu, { key: "Enter" });

    expect(onSelect).toHaveBeenCalledWith(2);
  });

  it("keyboard nav Escape calls onClose", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <ItemContextMenu x={100} y={100} onSelect={onSelect} onClose={onClose} />
    );

    const menu = screen.getByRole("menu");
    fireEvent.keyDown(menu, { key: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
