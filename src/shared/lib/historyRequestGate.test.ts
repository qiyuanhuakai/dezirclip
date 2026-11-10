import { expect, it } from "vitest";
import { createHistoryRequestGate } from "./historyRequestGate";

it("keeps a pending first-page refresh authoritative when pagination is requested", async () => {
  const gate = createHistoryRequestGate();
  gate.begin("all", true)!.finish(true, "all");
  let rows = [4, 3];
  let resolve!: (rows: number[]) => void;
  const response = new Promise<number[]>(done => { resolve = done; });
  const reset = gate.begin("all", true)!;
  const refresh = response.then(next => {
    if (reset.isCurrent("all")) rows = next;
    reset.finish(true, "all");
  });
  expect(gate.hasApplied("all")).toBe(true);
  expect(gate.canAppend("all")).toBe(false);
  expect(gate.begin("all", false)).toBeNull();
  resolve([5, 4]);
  await refresh;
  expect(rows).toEqual([5, 4]);
  expect(gate.canAppend("all")).toBe(true);
});

it("includes type and page size in scope even when the search text is unchanged", () => {
  const gate = createHistoryRequestGate();
  const all = JSON.stringify(["", null, 40]);
  const images = JSON.stringify(["", "image", 40]);
  gate.begin(all, true)!.finish(true, all);
  expect(gate.hasApplied(images)).toBe(false);
  expect(gate.begin(images, false)).toBeNull();
  const old = gate.begin(all, true)!;
  expect(old.isCurrent(images)).toBe(false);
  const current = gate.begin(images, true)!;
  old.finish(true, all);
  expect(gate.canAppend(images)).toBe(false);
  current.finish(true, images);
  expect(gate.canAppend(images)).toBe(true);
  expect(gate.canAppend(JSON.stringify(["", "image", 80]))).toBe(false);
});

it("rejects stale completion after privacy invalidation, failure or a newer refresh", () => {
  const gate = createHistoryRequestGate();
  const old = gate.begin("all", true)!;
  gate.invalidate();
  expect(old.isCurrent("all")).toBe(false);
  old.finish(true, "all");
  expect(gate.canAppend("all")).toBe(false);
  const failed = gate.begin("all", true)!;
  failed.finish(false, "all");
  expect(gate.canAppend("all")).toBe(false);
  const first = gate.begin("all", true)!;
  const second = gate.begin("all", true)!;
  first.finish(true, "all");
  expect(second.isCurrent("all")).toBe(true);
  expect(gate.canAppend("all")).toBe(false);
});
