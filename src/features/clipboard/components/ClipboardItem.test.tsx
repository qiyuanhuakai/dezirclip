import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import ClipboardItem from "./ClipboardItem";
import type { ClipboardEntry } from "../../../shared/types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn().mockReturnValue({
    outerPosition: vi.fn().mockResolvedValue({ x: 0, y: 0 }),
    outerSize: vi.fn().mockResolvedValue({ width: 800, height: 600 }),
    scaleFactor: vi.fn().mockResolvedValue(1),
  }),
  currentMonitor: vi.fn().mockResolvedValue(null),
  PhysicalPosition: class { constructor(public x: number, public y: number) {} },
  PhysicalSize: class { constructor(public width: number, public height: number) {} },
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: class {
    static getByLabel = vi.fn().mockResolvedValue(null);
  },
}));

vi.mock("../../../shared/lib/utils", () => ({
  getConciseTime: () => "刚刚",
  getTagColor: () => "#4a9eff",
}));

vi.mock("../../../shared/components/HtmlContent", () => ({
  default: ({ fallbackText }: { fallbackText: string }) => <span>{fallbackText}</span>,
}));

vi.mock("../../../shared/lib/localImageSrc", () => ({
  toTauriLocalImageSrc: () => null,
}));

vi.mock("../../../shared/lib/richPreview", () => ({
  extractRichImageFallback: () => ({ cleanHtml: null, imagePayload: null }),
  resolveRichImageSrc: () => null,
}));

vi.mock("../../../shared/lib/sourceAppIcon", () => ({
  getSourceAppIcon: vi.fn().mockResolvedValue(null),
  peekSourceAppIcon: () => null,
}));

vi.mock("../../../shared/lib/videoPreview", () => ({
  seekVideoPreviewFrame: vi.fn(),
}));

vi.mock("../../../shared/lib/contentTypeIcon", () => ({
  getContentTypeIcon: () => <span>icon</span>,
}));

vi.mock("../../../shared/lib/compactPreviewLog", () => ({
  compactPreviewLog: vi.fn(),
  richPreviewFailureLog: vi.fn(),
}));

vi.mock("../../../shared/lib/richTextSnapshot", () => ({
  getRichTextSnapshotDataUrl: () => null,
}));

vi.mock("./ItemContextMenu", () => ({
  ItemContextMenu: () => null,
}));

function makeEntry(overrides: Partial<ClipboardEntry> = {}): ClipboardEntry {
  return {
    id: 1,
    content_type: "image",
    content: "data:image/png;base64,AAAA",
    source_app: "test",
    timestamp: Date.now(),
    preview: "image",
    is_pinned: false,
    tags: [],
    ...overrides,
  };
}

const baseProps = {
  isSelected: false,
  windowPinned: false,
  isSensitiveHidden: false,
  isRevealed: false,
  isEditingTags: false,
  tagInput: "",
  theme: "mica",
  language: "zh" as const,
  t: (key: string) => {
    const map: Record<string, string> = {
      ocr_processing: "OCR 识别中...",
      ocr_done_label: "OCR 文本",
      ocr_failed_label: "OCR 失败",
      ocr_unsupported_label: "OCR 不支持",
      ocr_toggle_label: "展开/收起 OCR 文本",
      image_preview: "图片",
      chars: "字符",
      open: "打开",
      pin: "固定",
      unpin: "取消固定",
      delete: "删除",
      hide: "隐藏",
      reveal: "显示",
      pinned: "置顶",
      image_deleted: "图片已删除",
      file_deleted: "文件已删除",
      items: "个条目",
    };
    return map[key] || key;
  },
  onSelect: vi.fn(),
  onCopy: vi.fn(),
  onToggleReveal: vi.fn(),
  onOpen: vi.fn(),
  onTogglePin: vi.fn(),
  onDelete: vi.fn(),
  onToggleTagEditor: vi.fn(),
  onTagInput: vi.fn(),
  onTagAdd: vi.fn(),
  onTagDelete: vi.fn(),
};

describe("ClipboardItem OCR display", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("test_ocr_processing_badge_shown: item with status='processing' shows processing badge", async () => {
    const item = makeEntry({ ocr_status: "processing", ocr_text: null });
    await act(async () => {
      render(<ClipboardItem item={item} {...baseProps} />);
    });
    const badge = screen.getByTestId("ocr-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent("OCR 识别中...");
  });

  it("test_ocr_done_text_shown: item with status='done' and ocr_text shows the text", async () => {
    const item = makeEntry({ ocr_status: "done", ocr_text: "发票金额 ¥1234.56" });
    await act(async () => {
      render(<ClipboardItem item={item} {...baseProps} />);
    });
    const badge = screen.getByTestId("ocr-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent("OCR 文本");

    const toggle = screen.getByTestId("ocr-toggle");
    await act(async () => {
      toggle.click();
    });

    const ocrText = screen.getByTestId("ocr-text");
    expect(ocrText).toHaveTextContent("发票金额 ¥1234.56");
  });

  it("test_ocr_failed_badge_shown: item with status='failed' shows failed badge", async () => {
    const item = makeEntry({ ocr_status: "failed", ocr_text: null });
    await act(async () => {
      render(<ClipboardItem item={item} {...baseProps} />);
    });
    const badge = screen.getByTestId("ocr-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent("OCR 失败");
  });

  it("test_ocr_pending_no_badge: item with status='pending' shows no badge", async () => {
    const item = makeEntry({ ocr_status: "pending", ocr_text: null });
    await act(async () => {
      render(<ClipboardItem item={item} {...baseProps} />);
    });
    const badge = screen.queryByTestId("ocr-badge");
    expect(badge).not.toBeInTheDocument();
  });

  it("test_ocr_unsupported_badge_shown: item with status='unsupported' shows unsupported badge", async () => {
    const item = makeEntry({ ocr_status: "unsupported", ocr_text: null });
    await act(async () => {
      render(<ClipboardItem item={item} {...baseProps} />);
    });
    const badge = screen.getByTestId("ocr-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent("OCR 不支持");
  });
});
