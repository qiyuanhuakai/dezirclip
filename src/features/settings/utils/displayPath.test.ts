import { describe, expect, it } from "vitest";
import { displayPath } from "./displayPath";

describe("displayPath", () => {
    it("shows extended drive and UNC paths in familiar Windows syntax", () => {
        expect(displayPath(String.raw`\\?\C:\Apps\DezirClip\skills`)).toBe(String.raw`C:\Apps\DezirClip\skills`);
        expect(displayPath(String.raw`\\?\UNC\server\share\skills`)).toBe(String.raw`\\server\share\skills`);
    });
    it("preserves ordinary paths, device paths and the full filename", () => {
        for (const path of ["", "/opt/dezirclip/skills", String.raw`C:\Apps\skills`, String.raw`\\server\share\skills`, String.raw`\\?\Volume{123}\skills`]) {
            expect(displayPath(path)).toBe(path);
        }
        const path = String.raw`\\?\C:\a very long path\含中文目录\dezirclip-theme\SKILL.md`;
        expect(displayPath(path)).toBe(path.slice(4));
    });
});
