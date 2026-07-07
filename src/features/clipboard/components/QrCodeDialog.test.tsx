import { describe, it, expect, vi, beforeEach, beforeAll, afterAll } from "vitest";
import { render, fireEvent, screen, waitFor } from "@testing-library/react";
import QrCodeDialog from "./QrCodeDialog";
import type { ClipboardEntry } from "../../../shared/types";
import qrDialogCss from "./qrDialogCssLoader.mjs";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

const MOCK_DATA_URL = "data:image/png;base64,AAAA";
const MOCK_SVG = '<svg viewBox="0 0 100 100"></svg>';

function makeEntry(content: string): ClipboardEntry {
  return {
    id: 1,
    content_type: "text",
    content,
    source_app: "test",
    timestamp: Date.now(),
    preview: content,
    is_pinned: false,
    tags: [],
  };
}

describe("QrCodeDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "generate_qr_png") return MOCK_DATA_URL;
      if (cmd === "generate_qr_svg") return MOCK_SVG;
      return null;
    });
  });

  it("renders with entry content visible", async () => {
    const entry = makeEntry("https://example.com");
    render(<QrCodeDialog entry={entry} onClose={vi.fn()} />);

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("二维码")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("https://example.com")).toBeInTheDocument();
    });
  });

  it("close button calls onClose", async () => {
    const onClose = vi.fn();
    const entry = makeEntry("hello");
    render(<QrCodeDialog entry={entry} onClose={onClose} />);

    const closeBtn = screen.getByText("关闭");
    fireEvent.click(closeBtn);

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("copy SVG button writes SVG to clipboard", async () => {
    const entry = makeEntry("test content");
    render(<QrCodeDialog entry={entry} onClose={vi.fn()} />);

    const copyBtn = screen.getByText("复制 SVG");
    fireEvent.click(copyBtn);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("generate_qr_svg", {
        content: "test content",
      });
      expect(mockInvoke).toHaveBeenCalledWith("copy_to_clipboard", {
        content: MOCK_SVG,
        contentType: "text",
        paste: false,
        id: 0,
        deleteAfterUse: false,
        pasteWithFormat: false,
        moveToTop: false,
        pasteImageAsBase64: false,
      });
    });
  });

  it("ESC key closes dialog", async () => {
    const onClose = vi.fn();
    const entry = makeEntry("esc test");
    render(<QrCodeDialog entry={entry} onClose={onClose} />);

    fireEvent.keyDown(document, { key: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("clicking overlay backdrop closes dialog", async () => {
    const onClose = vi.fn();
    const entry = makeEntry("backdrop test");
    const { container } = render(
      <QrCodeDialog entry={entry} onClose={onClose} />
    );

    const overlay = container.querySelector(".qr-dialog-overlay");
    fireEvent.click(overlay!);

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("displays error when content is empty", async () => {
    const entry = makeEntry("");
    render(<QrCodeDialog entry={entry} onClose={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByText("内容不能为空")).toBeInTheDocument();
    });
    expect(mockInvoke).not.toHaveBeenCalledWith(
      "generate_qr_png",
      expect.anything()
    );
  });
});

describe("QrCodeDialog computed styles", () => {
  let styleEl: HTMLStyleElement;

  function resolveVar(
    expression: string,
    vars: Record<string, string>
  ): string {
    return expression.replace(
      /var\(\s*(--[^,\s)]+)\s*(?:,\s*([^)]+))?\s*\)/g,
      (_match, name: string, fallback?: string) => {
        const key = name.trim();
        const resolved = vars[key];
        if (resolved !== undefined && resolved !== "") return resolved;
        return fallback ? fallback.trim() : "";
      }
    );
  }

  function readVar(name: string): string {
    return getComputedStyle(document.documentElement)
      .getPropertyValue(name)
      .trim();
  }

  beforeAll(() => {
    styleEl = document.createElement("style");
    styleEl.textContent = qrDialogCss;
    document.head.appendChild(styleEl);
  });

  afterAll(() => {
    styleEl.remove();
  });

  beforeEach(() => {
    document.documentElement.style.removeProperty("--radius-window");
  });

  it("button borderRadius follows --radius-window (14px liquid-glass)", () => {
    document.documentElement.style.setProperty("--radius-window", "14px");
    const entry = makeEntry("https://example.com");
    const { container } = render(
      <QrCodeDialog entry={entry} onClose={vi.fn()} />
    );
    const btn = container.querySelector(
      ".qr-dialog__btn"
    ) as HTMLElement | null;
    expect(btn).not.toBeNull();
    const computedExpression = getComputedStyle(btn!).borderRadius;
    expect(computedExpression).toContain("var(--radius-window");
    const resolved = resolveVar(computedExpression, {
      "--radius-window": readVar("--radius-window"),
    });
    const radiusPx = parseFloat(resolved);
    expect(Number.isNaN(radiusPx)).toBe(false);
    expect(radiusPx.toFixed(2)).toBe("14.00");
  });
});
