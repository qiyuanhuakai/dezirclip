import { describe, expect, it } from "vitest";
import { attachmentKind, attachmentLayout, attachmentTiles } from "./attachmentPreview";

describe("attachment gallery projection", () => {
    it("fills multiple bounded filename rows before using an ellipsis", () => {
        const files = ['a.pdf', 'b.zip', 'c.xlsx', 'd.txt'].map(name => ({ name, src: null }));
        const result = attachmentLayout(files, 320, false);
        expect(result.columns).toBe(1);
        expect(result.visible).toHaveLength(4);
        expect(attachmentLayout(files, 160, false).visible).toHaveLength(4);
        expect(attachmentLayout(files, 320, false, true).visible).toHaveLength(4);
        const compact = attachmentLayout(files, 160, false, true);
        expect(compact.visible).toHaveLength(1);
        expect(compact.omitted).toBe(3);
        expect(files).toHaveLength(4);
        expect(attachmentLayout(files, 700, false).visible).toHaveLength(4);
    });
    it("gives a single video and a partial final filename row the unused width", () => {
        const files = Array.from({ length: 3 }, (_, i) => ({ name: `long-attachment-${i}.mp4`, src: null }));
        expect(attachmentLayout(files.slice(0, 1), 700, false, true).columns).toBe(1);
        const compact = attachmentLayout(files, 700, false, true);
        expect(compact.columns).toBe(2);
        expect(compact.lastSpan).toBe(2);
        expect(compact.omitted).toBe(0);
        expect(attachmentLayout(files, 700, false).columns).toBe(1);
    });
    it("bounds media and remaining names even on a wide screen", () => {
        const files = Array.from({ length: 30 }, (_, i) => ({ name: `${i}.png`, src: 'data:image/png;base64,AAAA' }));
        const result = attachmentLayout(files, 1200, false);
        expect(result.visible).toHaveLength(4);
        expect(result.names.length).toBeLessThanOrEqual(6);
        expect(result.visible.length + result.names.length + result.omitted).toBe(30);
        expect(attachmentLayout(files, 200, false).visible).toHaveLength(1);
        expect(attachmentLayout(files, 320, false, true).visible).toHaveLength(4);
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
