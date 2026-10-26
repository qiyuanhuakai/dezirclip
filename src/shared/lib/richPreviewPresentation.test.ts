import { describe, expect, it } from "vitest";
import { richPreviewPresentation } from "./richPreviewPresentation";

const bitmap = "data:image/png;base64,AQID";
describe("rich preview presentation", () => {
    it("keeps HTML text instead of replacing it with an offered bitmap", () => {
        expect(richPreviewPresentation("<p>Known mixed text</p>", bitmap)).toEqual({ html: true, appendFallback: true, containsImages: true });
    });
    it("preserves the existing embedded image without duplicating its bitmap fallback", () => {
        expect(richPreviewPresentation(`<p>Known mixed text</p><img src='${bitmap}'>`, bitmap)).toEqual({ html: true, appendFallback: false, containsImages: true });
    });
    it("keeps a bitmap-only fallback when HTML has no visible body", () => {
        expect(richPreviewPresentation("<style>p{color:red}</style><p>\u200b&nbsp;</p>", bitmap)).toEqual({ html: false, appendFallback: false, containsImages: true });
    });
    it("does not count remote resources as a RAM image or request them itself", () => {
        expect(richPreviewPresentation("<p>Known text</p><img src='https://example.invalid/image.png'>", bitmap)).toEqual({ html: true, appendFallback: true, containsImages: true });
        expect(richPreviewPresentation("<img src='file:///private/image.png'>", null)).toEqual({ html: false, appendFallback: false, containsImages: false });
    });
    it("keeps text-only rich content eligible for a snapshot", () => {
        expect(richPreviewPresentation("<p>Known text</p>", null)).toEqual({ html: true, appendFallback: false, containsImages: false });
    });
});
