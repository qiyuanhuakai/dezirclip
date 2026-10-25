import { describe, expect, it } from "vitest";
import type { ClipboardEntry } from "../types";
import { hasHistoryPreviewText, projectHistoryPreview, shouldLoadHistoryPreview } from "./historyPreview";

const row = (overrides: Partial<ClipboardEntry> = {}): ClipboardEntry => ({ id: 4, content_type: "text", content: "",
    preview: "", source_app: "Synthetic", timestamp: 1, is_pinned: false, tags: [], ...overrides });

describe("history metadata preview", () => {
    it("recognizes blank and invisible metadata without changing copied content", () => {
        for (const text of ["", " \n\t", "\u200b\u200e\ufeff", "\0\x1b"]) expect(hasHistoryPreviewText(text)).toBe(false);
        for (const text of ["文本", "👨‍👩‍👧", "  code\n", "https://example.invalid"]) expect(hasHistoryPreviewText(text)).toBe(true);
    });
    it("hydrates ordinary missing text summaries in compact and detailed views", () => {
        for (const kind of ["text", "code", "url", "rich_text"]) for (const compact of [true, false]) {
            expect(shouldLoadHistoryPreview(row({ content_type: kind }), compact)).toBe(true);
        }
    });
    it("keeps visible compact summaries local while preserving detailed rich hydration", () => {
        expect(shouldLoadHistoryPreview(row({ preview: "Visible summary" }), true)).toBe(false);
        const rich = row({ content_type: "rich_text", preview: "Rich summary" });
        expect(shouldLoadHistoryPreview(rich, true)).toBe(false);
        expect(shouldLoadHistoryPreview(rich, false)).toBe(true);
        expect(shouldLoadHistoryPreview({ ...rich, html_content: "<p>Rich summary</p>" }, false)).toBe(false);
    });
    it("never hydrates protected or unknown-tag entries automatically", () => {
        for (const tags of [["sensitive"], ["Password"], ["密码"]]) expect(shouldLoadHistoryPreview(row({ tags }), true)).toBe(false);
        expect(shouldLoadHistoryPreview(row({ tags: undefined }), false)).toBe(false);
        expect(() => projectHistoryPreview(row({ tags: ["sensitive"], content: "Protected body" }))).toThrow("ProtectedPreviewRequiresReveal");
    });
    it("does not duplicate media thumbnail reads or fetch unknown kinds", () => {
        for (const kind of ["image", "video", "file", "files", "unknown"]) expect(shouldLoadHistoryPreview(row({ content_type: kind }), true)).toBe(false);
    });
    it("builds a bounded summary from the authorized body and leaves the input intact", () => {
        const input = row({ content: " \n" + "😀".repeat(2300), html_content: "<p>Rich body</p>" });
        const projected = projectHistoryPreview(input);
        expect([...projected.preview]).toHaveLength(500);
        expect(projected.preview).toBe("😀".repeat(500));
        expect(projected).not.toHaveProperty("content");
        expect(projected.html_content).toBe(input.html_content);
        expect(input.preview).toBe("");
        expect(input.content).toBe(" \n" + "😀".repeat(2300));
    });
    it("preserves a supplied readable summary and legitimate whitespace-only body", () => {
        expect(projectHistoryPreview(row({ preview: "Summary", content: "Different full body" })).preview).toBe("Summary");
        expect(hasHistoryPreviewText(projectHistoryPreview(row({ content: " \n\t " })).preview)).toBe(false);
    });
    it("skips invisible leading layout characters without losing the real summary to the limit", () => {
        const leading = "\u200b\u200e\n ".repeat(600);
        expect(projectHistoryPreview(row({ content: leading + "Actual body" })).preview).toBe("Actual body");
        expect(projectHistoryPreview(row({ preview: leading + "Actual summary" })).preview).toBe("Actual summary");
    });
    it("preserves HTML-only resources for the existing safe HTML renderer", () => {
        const html = "\u200b<p>Known valid rich text</p>";
        const projected = projectHistoryPreview(row({ content_type: "rich_text", html_content: html }));
        expect(projected.html_content).toBe(html);
        expect(projected).not.toHaveProperty("content");
    });
    it("hydrates and skips default-ignorable characters without dropping meaningful emoji", () => {
        for (const hidden of ["\ufe0f", "\u034f", "\u3164", "\ufe0f\u034f\u3164"]) {
            expect(hasHistoryPreviewText(hidden)).toBe(false);
            expect(shouldLoadHistoryPreview(row({ preview: hidden }), true)).toBe(true);
            expect(projectHistoryPreview(row({ content: hidden + "Visible body" })).preview).toBe("Visible body");
            expect(projectHistoryPreview(row({ content: hidden })).preview).toBe("");
        }
        expect(projectHistoryPreview(row({ content: "❤️ Visible body" })).preview).toBe("❤️ Visible body");
    });
});
