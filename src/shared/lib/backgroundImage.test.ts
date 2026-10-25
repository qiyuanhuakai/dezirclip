import { describe, expect, it } from "vitest";
import { decodeBackgroundImage } from "./backgroundImage";

describe("bounded background image response", () => {
  it("preserves the MIME and exact decoded image bytes", () => {
    const value = decodeBackgroundImage("data:image/png;base64,iVBORw0KGgo=");
    expect(value.mime).toBe("image/png");
    expect(Array.from(value.bytes)).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
    for (const mime of ["jpeg", "gif", "webp"]) expect(decodeBackgroundImage(`data:image/${mime};base64,AQID`).mime).toBe(`image/${mime}`);
  });

  it("rejects executable formats, malformed data, and empty images", () => {
    for (const value of ["data:image/svg+xml;base64,AQID", "https://example.invalid/image.png", "data:image/png;base64,", "data:image/png;base64,A", "data:image/png;base64,A===", "data:image/png;base64,AQ ID"]) {
      expect(() => decodeBackgroundImage(value)).toThrow("CustomBackgroundInvalidImage");
    }
  });

  it("preserves payloads larger than the CSS custom-property limit without changing bytes", () => {
    const source = new Uint8Array(2 * 1024 * 1024 + 257).fill(97);
    const value = decodeBackgroundImage(`data:image/png;base64,${btoa("a".repeat(source.length))}`);
    expect(value.bytes.length).toBe(source.length);
    expect(value.bytes.every(byte => byte === 97)).toBe(true);
  });

  it("rejects responses exceeding the backend byte limit", () => {
    expect(() => decodeBackgroundImage(`data:image/png;base64,${"A".repeat(Math.ceil(16 * 1024 * 1024 / 3) * 4 + 68)}`)).toThrow("CustomBackgroundInvalidImage");
  });
});
