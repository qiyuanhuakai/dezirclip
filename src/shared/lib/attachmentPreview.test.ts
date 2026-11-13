import { describe, expect, it } from "vitest";
import { attachmentTiles } from "./attachmentPreview";

describe("attachment gallery projection", () => {
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
