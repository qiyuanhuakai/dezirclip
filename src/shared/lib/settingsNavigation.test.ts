import { describe, expect, it } from "vitest";
import { createSettingsNavigation, PRIVACY_RULES_DESTINATION } from "./settingsNavigation";

describe("settings navigation requests", () => {
  it("queues the exact destination until a lazy settings panel consumes it", () => {
    const queue = createSettingsNavigation();
    expect(queue.request({ destination: PRIVACY_RULES_DESTINATION })).toBe(true);
    expect(queue.take()).toEqual({ id: 1, destination: PRIVACY_RULES_DESTINATION });
    expect(queue.take()).toBeNull();
  });
  it("notifies a mounted panel and allows repeated navigation", () => {
    const queue = createSettingsNavigation();
    const seen: number[] = [];
    queue.subscribe(() => { const item = queue.take(); if (item) seen.push(item.id); });
    queue.request({ destination: PRIVACY_RULES_DESTINATION });
    queue.request({ destination: PRIVACY_RULES_DESTINATION });
    expect(seen).toEqual([1, 2]);
  });
  it("rejects unrelated events and never accepts arbitrary paths or groups", () => {
    const queue = createSettingsNavigation();
    for (const value of [undefined, null, "", [], {}, { destination: "data" }, { destination: "../private" }]) {
      expect(queue.request(value)).toBe(false);
    }
    expect(queue.take()).toBeNull();
  });
  it("removes a listener while retaining a later request for remount", () => {
    const queue = createSettingsNavigation();
    let calls = 0;
    const stop = queue.subscribe(() => { calls += 1; });
    stop();
    queue.request({ destination: PRIVACY_RULES_DESTINATION });
    expect(calls).toBe(0);
    expect(queue.take()?.id).toBe(1);
  });
});
