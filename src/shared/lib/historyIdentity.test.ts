import { describe, expect, it } from "vitest";
import { entryIdentityArgs, entryInstanceArgs, matchesEntryIdentity } from "./historyIdentity";
import type { ClipboardEntry } from "../types";

const entry = (id: number, hostInstanceId?: string): ClipboardEntry => ({
  id, host_instance_id: hostInstanceId, content: "", content_type: "text", source_app: "",
  timestamp: 0, preview: "", is_pinned: false, tags: [],
});

describe("history host identity", () => {
  it("forwards the selected summary's original instance with its session id", () => {
    const selected = entry(-7, "original-host");
    expect(entryIdentityArgs(selected)).toEqual({ id: -7, expectedHostInstanceId: "original-host" });
    expect(entryIdentityArgs(selected)).not.toEqual(entryIdentityArgs(entry(-7, "restarted-host")));
  });

  it("keeps absent identity absent so the backend can reject an unbound session id", () => {
    expect(entryIdentityArgs(entry(-7))).toEqual({ id: -7 });
    expect(entryIdentityArgs(entry(7))).toEqual({ id: 7 });
    expect(entryInstanceArgs()).toEqual({});
  });

  it("does not apply a late result to the same numeric id in another host", () => {
    expect(matchesEntryIdentity(entry(-7, "original-host"), -7, "original-host")).toBe(true);
    expect(matchesEntryIdentity(entry(-7, "restarted-host"), -7, "original-host")).toBe(false);
    expect(matchesEntryIdentity(entry(-7, "restarted-host"), -7)).toBe(false);
    expect(matchesEntryIdentity(entry(7), 7)).toBe(true);
  });
});
