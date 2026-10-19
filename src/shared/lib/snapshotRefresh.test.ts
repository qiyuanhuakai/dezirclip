import { describe, expect, it } from "vitest";
import { createSnapshotRefresh } from "./snapshotRefresh";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

describe("snapshot refresh", () => {
  it("coalesces bulk reloads and publishes only the latest snapshot", async () => {
    const first = deferred<string[]>();
    const second = deferred<string[]>();
    const output: string[][] = [];
    let reads = 0;
    const reader = createSnapshotRefresh(() => ++reads === 1 ? first.promise : second.promise, value => output.push(value), () => {});
    const pending = reader.refresh();
    reader.refresh();
    reader.refresh();
    first.resolve(["old tag"]);
    await Promise.resolve();
    expect(output).toEqual([]);
    expect(reads).toBe(2);
    second.resolve(["renamed tag"]);
    await pending;
    expect(output).toEqual([["renamed tag"]]);
  });

  it("preserves live additions or removals while a bulk reload is pending", async () => {
    const first = deferred<number[]>();
    const second = deferred<number[]>();
    let reads = 0;
    let entries = [1, 2];
    const reader = createSnapshotRefresh(() => ++reads === 1 ? first.promise : second.promise, value => { entries = value; }, () => {});
    const pending = reader.refresh();
    entries = [3, 1];
    reader.invalidate();
    first.resolve([1, 2]);
    await Promise.resolve();
    expect(entries).toEqual([3, 1]);
    second.resolve([3, 1]);
    await pending;
    expect(entries).toEqual([3, 1]);
    reader.invalidate();
    expect(reads).toBe(2);
  });

  it("cancels an unmounted reader and allows a fresh lifecycle to reload", async () => {
    const first = deferred<number>();
    const output: number[] = [];
    let reads = 0;
    const reader = createSnapshotRefresh(() => ++reads === 1 ? first.promise : Promise.resolve(2), value => output.push(value), () => {});
    const pending = reader.refresh();
    reader.cancel();
    first.resolve(1);
    await pending;
    expect(output).toEqual([]);
    await reader.refresh();
    expect(output).toEqual([2]);
  });

  it("does not clear newer live state for an obsolete failure and recovers from a current failure", async () => {
    const first = deferred<number>();
    const output: number[] = [];
    let reads = 0;
    let errors = 0;
    const reader = createSnapshotRefresh(() => {
      reads += 1;
      if (reads === 1) return first.promise;
      if (reads === 3) return Promise.reject(new Error("current failure"));
      return Promise.resolve(reads);
    }, value => output.push(value), () => { errors += 1; });
    const pending = reader.refresh();
    reader.invalidate();
    first.reject(new Error("obsolete failure"));
    await pending;
    expect(output).toEqual([2]);
    expect(errors).toBe(0);
    await reader.refresh();
    expect(errors).toBe(1);
    await reader.refresh();
    expect(output).toEqual([2, 4]);
  });
});
