import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, screen, waitFor } from "@testing-library/react";
import { ItemContextMenu } from "./ItemContextMenu";
import type { ClipboardEntry } from "../../../shared/types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const MOCK_TRANSFORM_KINDS = [
  { id: "to_uppercase", label_zh: "转大写", label_en: "To Uppercase" },
  { id: "to_lowercase", label_zh: "转小写", label_en: "To Lowercase" },
  { id: "title_case", label_zh: "首字母大写", label_en: "Title Case" },
  { id: "sentence_case", label_zh: "句首大写", label_en: "Sentence Case" },
  { id: "trim", label_zh: "去除空格", label_en: "Trim" },
  { id: "collapse_spaces", label_zh: "合并连续空格", label_en: "Collapse Spaces" },
  { id: "remove_newlines", label_zh: "移除换行", label_en: "Remove Newlines" },
  { id: "sort_asc", label_zh: "升序排序", label_en: "Sort Asc" },
  { id: "sort_desc", label_zh: "降序排序", label_en: "Sort Desc" },
  { id: "dedupe", label_zh: "去重行", label_en: "Dedupe" },
  { id: "reverse_lines", label_zh: "反转行序", label_en: "Reverse Lines" },
  { id: "reverse_text", label_zh: "反转文本", label_en: "Reverse Text" },
  { id: "line_numbers", label_zh: "添加行号", label_en: "Line Numbers" },
  { id: "url_encode", label_zh: "URL 编码", label_en: "URL Encode" },
  { id: "url_decode", label_zh: "URL 解码", label_en: "URL Decode" },
  { id: "base64_encode", label_zh: "Base64 编码", label_en: "Base64 Encode" },
  { id: "base64_decode", label_zh: "Base64 解码", label_en: "Base64 Decode" },
];

function makeEntry(content: string, isPinned = false): ClipboardEntry {
  return {
    id: 1,
    content_type: "text",
    content,
    source_app: "test",
    timestamp: Date.now(),
    preview: content,
    is_pinned: isPinned,
    tags: [],
  };
}

describe("ItemContextMenu", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders seven menu items", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <ItemContextMenu x={100} y={100} onSelect={onSelect} onClose={onClose} />
    );

    const items = screen.getAllByRole("menuitem");
    expect(items).toHaveLength(7);
    expect(items[0]).toHaveTextContent("复制");
    expect(items[1]).toHaveTextContent("编辑标签");
    expect(items[2]).toHaveTextContent("QR 码");
    expect(items[3]).toHaveTextContent("删除");
    expect(items[4]).toHaveTextContent("固定");
    expect(items[5]).toHaveTextContent("分享");
    expect(items[6]).toHaveTextContent("文本转换 →");
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
    expect(items[0].className).not.toContain("item-context-menu__item--active");
    expect(items[1].className).toContain("item-context-menu__item--active");
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

    expect(onSelect).toHaveBeenCalledWith("qrCode");
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

  it("click copy invokes onCopy callback", () => {
    const onCopy = vi.fn();
    const onClose = vi.fn();
    const onSelect = vi.fn();
    render(
      <ItemContextMenu
        x={100}
        y={100}
        onSelect={onSelect}
        onClose={onClose}
        onCopy={onCopy}
      />
    );

    const items = screen.getAllByRole("menuitem");
    fireEvent.click(items[0]);
    expect(onCopy).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("click QR 码 invokes onQRCode callback", () => {
    const onQRCode = vi.fn();
    const onClose = vi.fn();
    const onSelect = vi.fn();
    render(
      <ItemContextMenu
        x={100}
        y={100}
        onSelect={onSelect}
        onClose={onClose}
        onQRCode={onQRCode}
      />
    );

    const items = screen.getAllByRole("menuitem");
    fireEvent.click(items[2]);
    expect(onQRCode).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("hover transforms opens submenu with 17 transform kinds", async () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <ItemContextMenu
        x={100}
        y={100}
        onSelect={onSelect}
        onClose={onClose}
        transformKinds={MOCK_TRANSFORM_KINDS}
      />
    );

    const menu = screen.getByRole("menu");
    for (let i = 0; i < 6; i++) {
      fireEvent.keyDown(menu, { key: "ArrowDown" });
    }
    fireEvent.keyDown(menu, { key: "ArrowRight" });

    await waitFor(() => {
      const submenu = screen.getByTestId("transform-submenu");
      expect(submenu.className).toContain("item-context-menu__submenu--open");
    });

    const transformItems = screen.getAllByTestId("transform-item");
    expect(transformItems).toHaveLength(17);
    expect(transformItems[0]).toHaveTextContent("转大写");
    expect(transformItems[1]).toHaveTextContent("转小写");
  });

  it("click transform item invokes onTransform with correct kind", async () => {
    const onTransform = vi.fn();
    const onClose = vi.fn();
    const onSelect = vi.fn();
    render(
      <ItemContextMenu
        x={100}
        y={100}
        onSelect={onSelect}
        onClose={onClose}
        onTransform={onTransform}
        transformKinds={MOCK_TRANSFORM_KINDS}
      />
    );

    const menu = screen.getByRole("menu");
    for (let i = 0; i < 6; i++) {
      fireEvent.keyDown(menu, { key: "ArrowDown" });
    }
    fireEvent.keyDown(menu, { key: "ArrowRight" });

    await waitFor(() => {
      const submenu = screen.getByTestId("transform-submenu");
      expect(submenu.className).toContain("item-context-menu__submenu--open");
    });

    const firstTransform = screen.getAllByTestId("transform-item")[0];
    fireEvent.click(firstTransform);

    expect(onTransform).toHaveBeenCalledWith("to_uppercase");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows unpin label when entry is pinned", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    const entry = makeEntry("test", true);
    render(
      <ItemContextMenu
        x={100}
        y={100}
        entry={entry}
        onSelect={onSelect}
        onClose={onClose}
      />
    );

    const items = screen.getAllByRole("menuitem");
    expect(items[4]).toHaveTextContent("取消固定");
  });
});
