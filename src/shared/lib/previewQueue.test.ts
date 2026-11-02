import { expect, it } from "vitest";
import { createPreviewQueue } from "./previewQueue";

it("bounds simultaneous thumbnail reads and drops rows unmounted during a scroll", async () => {
  const queue = createPreviewQueue(2);
  const visible = new AbortController();
  const removed = new AbortController();
  const started: number[] = [];
  const done: Array<() => void> = [];
  const request = (id: number, signal = visible.signal) => queue(signal, () => new Promise<void>(resolve => {
    started.push(id);
    done.push(resolve);
  }));
  const first = request(1), second = request(2);
  const discarded = expect(request(3, removed.signal)).rejects.toThrow("PreviewCancelled");
  const last = request(4);
  removed.abort();
  await discarded;
  expect(started).toEqual([1, 2]);
  done[0]();
  await first;
  await Promise.resolve();
  expect(started).toEqual([1, 2, 4]);
  done[1](); done[2]();
  await Promise.all([second, last]);
});
