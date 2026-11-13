import { describe, expect, it } from "vitest";
import { attachmentKind, attachmentLayout, attachmentTiles } from "./attachmentPreview";

describe("attachment gallery projection", () => {
    it("summarizes a file group instead of silently dropping files that do not fit", () => {
        const files = ['a.pdf', 'b.zip', 'c.xlsx', 'd.txt'].map(name => ({ name, src: null }));
        const result = attachmentLayout(files, 320, false);
        expect(result.summary).toBe(true);
        expect(result.visible).toEqual([]);
        expect(files).toHaveLength(4);
        expect(attachmentLayout(files, 700, false).visible).toHaveLength(4);
    });
    it("bounds media and remaining names even on a wide screen", () => {
        const files = Array.from({ length: 30 }, (_, i) => ({ name: `${i}.png`, src: 'data:image/png;base64,AAAA' }));
        const result = attachmentLayout(files, 1200, false);
        expect(result.visible).toHaveLength(4);
        expect(result.names.length).toBeLessThanOrEqual(6);
        expect(result.visible.length + result.names.length + result.omitted).toBe(30);
        expect(attachmentLayout(files, 200, false).visible).toHaveLength(1);
    });
    it("classifies file extensions without opening files", () => {
        expect(['paper.PDF', 'book.xlsx', 'archive.zip', 'clip.mp4', 'song.flac', 'main.rs', 'unknown'].map(attachmentKind))
            .toEqual(['document', 'spreadsheet', 'archive', 'video', 'audio', 'code', 'file']);
    });
    it("preserves attachment order and names alongside individual thumbnails", () => {
        expect(attachmentTiles('<figure><img src="data:image/png;base64,AAAA" alt="a"><figcaption>a &amp; b.png</figcaption></figure><figure><figcaption>notes.txt</figcaption></figure>', "")).toEqual([
            { name: "a & b.png", src: "data:image/png;base64,AAAA" }, { name: "notes.txt", src: null },
        ]);
    });
    it("never accepts remote images, event handlers or other markup", () => {
        for (const html of ['<figure><img src="https://example.invalid/x"><figcaption>remote</figcaption></figure>',
            '<figure><img src="data:image/png;base64,AAAA" onerror="run()"><figcaption>x</figcaption></figure>',
            '<script>run()</script><figure><figcaption>x</figcaption></figure>']) {
            expect(attachmentTiles(html, "fallback.mp4")).toEqual([{ name: "fallback.mp4", src: null }]);
        }
    });
    it("shows every filename when previews are missing", () => {
        expect(attachmentTiles(null, "C:\\files\\one.png\n/home/user/two.txt").map(value => value.name)).toEqual(["one.png", "two.txt"]);
    });
});
