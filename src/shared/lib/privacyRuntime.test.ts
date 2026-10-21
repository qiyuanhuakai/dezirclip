import { describe, expect, it } from "vitest";
import { concealSensitiveEntry, createPrivacyFence, isRamImageSource, isSensitiveEntry, parsePrivacySnapshot } from "./privacyRuntime";

const snapshot = () => ({ history_revision: 3, privacy_epoch: 7, session: { active: true, expires_at_ms: null } });
describe("privacy runtime boundary", () => {
  it("only accepts RAM image representations rather than reusable file addresses", () => {
    expect(isRamImageSource("data:image/png;base64,AAAA")).toBe(true);
    for (const source of ["/private/image.png", "C:\\private\\image.png", "file:///tmp/image.png", "asset://private/image.png",
      "http://asset.localhost/image.png", "https://example.org/image.png", "data:image/svg+xml;base64,AAAA", "data:image/png;base64,<bad>"])
      expect(isRamImageSource(source)).toBe(false);
  });
  it("accepts validated counters and future optional fields without changing known fields", () => {
    expect(parsePrivacySnapshot({ ...snapshot(), future: "optional" })).toEqual(snapshot());
    expect(parsePrivacySnapshot({ ...snapshot(), session: { active: true, expires_at_ms: 12345 } }).session.expires_at_ms).toBe(12345);
  });
  it("rejects missing, malformed and unsafe state instead of inventing protection", () => {
    for (const value of [null, {}, { ...snapshot(), privacy_epoch: true }, { ...snapshot(), history_revision: -1 },
      { ...snapshot(), privacy_epoch: Number.MAX_SAFE_INTEGER + 1 }, { ...snapshot(), privacy_epoch: NaN },
      { ...snapshot(), session: { active: "true", expires_at_ms: null } },
      { ...snapshot(), session: { active: false } }, { ...snapshot(), session: { active: true, expires_at_ms: -1 } }]) {
      expect(() => parsePrivacySnapshot(value)).toThrow("PrivacyStateUnavailable");
    }
  });
  it("keeps protected tags hidden regardless of the classifier setting", () => {
    for (const tag of ["sensitive", " PASSWORD ", "密码"]) expect(isSensitiveEntry({ tags: [tag] })).toBe(true);
    expect(isSensitiveEntry({ tags: ["email", "phone", "ordinary"] })).toBe(false);
  });
  it("removes body, rich HTML, OCR and private image paths before concealed DOM rendering", () => {
    const entry = { id: 1, content_type: "image", content: "/private/synthetic.png", html_content: "<p>synthetic secret</p>",
      preview: "synthetic secret", ocr_text: "synthetic OCR", ocr_status: "done", source_app: "fixture", timestamp: 1,
      is_pinned: false, tags: ["sensitive"], use_count: 0 };
    const concealed = concealSensitiveEntry(entry);
    expect(concealed.content).toBe("");
    expect(concealed.html_content).toBeUndefined();
    expect(concealed.ocr_text).toBeNull();
    expect(concealed.preview).not.toContain("synthetic secret");
    expect(entry.content).toBe("/private/synthetic.png");
    expect(concealed.tags).toEqual(["sensitive"]);
  });
  it("rejects delayed results after on/off transitions and tokens from another window scope", async () => {
    const fence = createPrivacyFence();
    const before = fence.capture();
    const delayed = Promise.resolve().then(() => fence.isCurrent(before));
    fence.revoke();
    fence.revoke();
    expect(await delayed).toBe(false);
    expect(fence.isCurrent(fence.capture())).toBe(true);
    expect(fence.isCurrent(createPrivacyFence().capture())).toBe(false);
  });
});
