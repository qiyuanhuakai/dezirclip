import { describe, expect, it } from "vitest";
import type { ClipboardEntry } from "../types";
import { createResumePage } from "./resumePage";

const stamp = { instanceId: "a", epoch: 3, revision: 19, sessionActive: false };
const row: ClipboardEntry = { id: 42, host_instance_id: "a", content_type: "text", content: "public", source_app: "fixture",
  timestamp: 1, preview: "public", is_pinned: false, tags: [] };

describe("hidden first page", () => {
  it("reuses only the same host, policy, history revision and query", () => {
    const page = createResumePage();
    for (const changed of [{ instanceId: "b" }, { epoch: 4 }, { revision: 20 }, { sessionActive: true }]) {
      page.save(stamp, "all", [row]);
      expect(page.read({ ...stamp, ...changed }, "all")).toBeNull();
    }
    page.save(stamp, "all", [row]);
    expect(page.read(stamp, "image")).toBeNull();
  });
  it("keeps the original projection isolated from UI edits and discards it on revocation", () => {
    const page = createResumePage();
    const input = { ...row, tags: ["sample"] };
    page.save(stamp, "all", [input]);
    input.tags.push("changed");
    page.read(stamp, "all")![0].tags.push("other");
    expect(page.read(stamp, "all")![0].tags).toEqual(["sample"]);
    page.clear();
    expect(page.read(stamp, "all")).toBeNull();
  });
  it("does not retain session records, oversized bodies or opened sensitive details", () => {
    const page = createResumePage();
    for (const changed of [{ id: -1 }, { content: "a".repeat(100_000) }, { tags: ["sensitive"] }, { host_instance_id: "b" }]) {
      page.save(stamp, "all", [{ ...row, ...changed }]);
      expect(page.read(stamp, "all")).toBeNull();
    }
  });
});
