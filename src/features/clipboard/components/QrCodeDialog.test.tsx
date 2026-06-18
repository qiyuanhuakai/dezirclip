import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, screen, waitFor } from "@testing-library/react";
import QrCodeDialog from "./QrCodeDialog";
import type { ClipboardEntry } from "../../../shared/types";

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
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    const entry = makeEntry("test content");
    render(<QrCodeDialog entry={entry} onClose={vi.fn()} />);

    const copyBtn = screen.getByText("复制 SVG");
    fireEvent.click(copyBtn);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("generate_qr_svg", {
        content: "test content",
      });
      expect(writeText).toHaveBeenCalledWith(MOCK_SVG);
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
