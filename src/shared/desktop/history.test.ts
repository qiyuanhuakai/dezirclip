import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { desktopHistory } from "./history";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const access = { epoch: 12, isCurrent: () => true };
const query = { limit: 20, offset: 4, kind: "image" };
const tagQuery = { tag: "work", query: "input", sortBy: "time" as const, limit: 30, offset: 10 };

afterEach(() => {
  vi.resetAllMocks();
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

describe("desktop history reads", () => {
  it("uses Tauri argument names and forwards the captured epoch for tagged reads", async () => {
    vi.stubEnv("MODE", "production");
    await desktopHistory.page(access, query);
    await desktopHistory.search(access, "fixture", 40);
    await desktopHistory.tagPage(access, tagQuery);
    await desktopHistory.tagCounts(access);
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["get_clipboard_history", { limit: 20, offset: 4, contentType: "image" }],
      ["search_clipboard_history", { searchTerm: "fixture", limit: 40 }],
      ["get_tag_items_page", { ...tagQuery, expectedEpoch: 12 }],
      ["get_all_tags_info"],
    ]);
  });

  it("uses MyGo's existing library bindings, epoch and hosted row identity", async () => {
    vi.stubEnv("MODE", "mygo");
    const rows = [{ id: -4, host_instance_id: "current-host" }];
    const call = vi.fn().mockResolvedValue(rows);
    vi.stubGlobal("mygo", { call });
    expect(await desktopHistory.page(access, query)).toBe(rows);
    await desktopHistory.search(access, "fixture", 40);
    await desktopHistory.tagPage(access, tagQuery);
    await desktopHistory.tagCounts(access);
    expect(call.mock.calls).toEqual([
      ["Library.Page", { limit: 20, offset: 4, kind: "image" }, 12],
      ["Library.Page", { limit: 40, offset: 0, query: "fixture" }, 12],
      ["Library.TagPage", { tag: "work", query: "input", limit: 30, offset: 10, sort_by: "time", expected_epoch: 12 }],
      ["Library.TagCounts", 12],
    ]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("refuses a missing or retired capability and rejects data arriving after revocation", async () => {
    vi.stubEnv("MODE", "mygo");
    let current = true;
    const call = vi.fn(async () => { current = false; return []; });
    vi.stubGlobal("mygo", { call });
    await expect(desktopHistory.page(null, query)).rejects.toThrow("PrivacyStateUnavailable");
    await expect(desktopHistory.tagCounts({ epoch: 11, isCurrent: () => false })).rejects.toThrow("PrivacyEpochChanged");
    expect(call).not.toHaveBeenCalled();
    await expect(desktopHistory.page({ epoch: 12, isCurrent: () => current }, query)).rejects.toThrow("PrivacyEpochChanged");
    expect(call).toHaveBeenCalledTimes(1);
  });
});
