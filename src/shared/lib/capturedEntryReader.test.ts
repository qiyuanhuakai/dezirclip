// @vitest-environment node
import { describe, expect, it } from "vitest";
import { createCapturedEntryReader } from "./capturedEntryReader";

type Summary = { id: number; preview: string };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function controlledReader() {
  const requests: ({ id: number } & ReturnType<typeof deferred<Summary>>)[] = [];
  const published: Summary[] = [];
  let refreshes = 0;
  const reader = createCapturedEntryReader((id) => {
    const request = { id, ...deferred<Summary>() };
    requests.push(request);
    return request.promise;
  }, (entry) => published.push(entry), () => { refreshes += 1; });
  return { reader, requests, published, get refreshes() { return refreshes; } };
}

const summary = (id: number, preview = `synthetic ${id}`): Summary => ({ id, preview });

async function flushResponses() {
  await Promise.resolve();
  await Promise.resolve();
}

describe("captured entry reader", () => {
  it("publishes completed sequential captures in order without redundant snapshots", async () => {
    const state = controlledReader();
    state.reader.capture(1);
    expect(state.published).toEqual([]);
    state.requests[0].resolve(summary(1));
    await flushResponses();
    state.reader.capture(-2);
    expect(state.requests.map(({ id }) => id)).toEqual([1, -2]);
    state.requests[1].resolve(summary(-2));
    await flushResponses();
    expect(state.published).toEqual([summary(1), summary(-2)]);
    expect(state.refreshes).toBe(0);
    expect(state.reader.invalidate()).toBe(false);
  });

  it("keeps a blocked thousand-event burst to the first and latest read, then reconciles gaps", async () => {
    const state = controlledReader();
    for (let id = 1; id <= 1000; id += 1) state.reader.capture(id);
    expect(state.requests.map(({ id }) => id)).toEqual([1]);
    expect(state.refreshes).toBe(0);
    state.requests[0].resolve(summary(1));
    await flushResponses();
    expect(state.requests.map(({ id }) => id)).toEqual([1, 1000]);
    expect(state.published).toEqual([]);
    expect(state.refreshes).toBe(0);
    state.requests[1].resolve(summary(1000));
    await flushResponses();
    expect(state.published).toEqual([summary(1000)]);
    expect(state.refreshes).toBe(1);
  });

  it("reads a newer same-id notification again and never publishes its older response", async () => {
    const state = controlledReader();
    state.reader.capture(7);
    state.reader.capture(7);
    state.requests[0].resolve(summary(7, "older preview"));
    await flushResponses();
    expect(state.requests.map(({ id }) => id)).toEqual([7, 7]);
    expect(state.published).toEqual([]);
    state.requests[1].resolve(summary(7, "newer preview"));
    await flushResponses();
    expect(state.published).toEqual([summary(7, "newer preview")]);
    expect(state.refreshes).toBe(1);
  });

  it("continues a burst after an obsolete rejection and refreshes only after the latest success", async () => {
    const state = controlledReader();
    state.reader.capture(1);
    state.reader.capture(2);
    state.requests[0].reject(new Error("obsolete read failed"));
    await flushResponses();
    expect(state.requests.map(({ id }) => id)).toEqual([1, 2]);
    expect(state.refreshes).toBe(0);
    state.requests[1].resolve(summary(2));
    await flushResponses();
    expect(state.published).toEqual([summary(2)]);
    expect(state.refreshes).toBe(1);
  });

  it("refreshes on the latest rejection without publishing an earlier success, and can recover", async () => {
    const state = controlledReader();
    state.reader.capture(1);
    state.reader.capture(2);
    state.requests[0].resolve(summary(1));
    await flushResponses();
    state.requests[1].reject(new Error("latest read failed"));
    await flushResponses();
    expect(state.published).toEqual([]);
    expect(state.refreshes).toBe(1);
    state.reader.capture(3);
    state.requests[2].resolve(summary(3));
    await flushResponses();
    expect(state.published).toEqual([summary(3)]);
    expect(state.refreshes).toBe(1);
  });

  it("treats synchronous read failure as a failed hydration and accepts a later capture", async () => {
    const published: Summary[] = [];
    let refreshes = 0;
    const reader = createCapturedEntryReader<Summary>((id) => {
      if (id === 1) throw new Error("bridge unavailable");
      return Promise.resolve(summary(id));
    }, (entry) => published.push(entry), () => { refreshes += 1; });
    reader.capture(1);
    await flushResponses();
    expect(published).toEqual([]);
    expect(refreshes).toBe(1);
    reader.capture(2);
    await flushResponses();
    expect(published).toEqual([summary(2)]);
    expect(refreshes).toBe(1);
  });

  it("rejects a mismatched response id and refreshes instead of publishing another entry", async () => {
    const state = controlledReader();
    state.reader.capture(4);
    state.requests[0].resolve(summary(5));
    await flushResponses();
    expect(state.published).toEqual([]);
    expect(state.refreshes).toBe(1);
  });

  it("invalidates active and queued captures after a caller's authoritative edit or removal", async () => {
    const state = controlledReader();
    state.reader.capture(1);
    state.reader.capture(2);
    expect(state.reader.invalidate()).toBe(true);
    state.requests[0].resolve(summary(1));
    await flushResponses();
    expect(state.requests.map(({ id }) => id)).toEqual([1]);
    expect(state.published).toEqual([]);
    expect(state.refreshes).toBe(0);
    expect(state.reader.invalidate()).toBe(false);
    state.reader.capture(3);
    state.requests[1].resolve(summary(3));
    await flushResponses();
    expect(state.published).toEqual([summary(3)]);
  });

  it("accepts a fresh capture while an invalidated request remains blocked", async () => {
    const state = controlledReader();
    state.reader.capture(1);
    state.reader.capture(2);
    state.reader.invalidate();
    state.reader.capture(3);
    expect(state.requests.map(({ id }) => id)).toEqual([1]);
    state.requests[0].reject(new Error("invalidated request failed late"));
    await flushResponses();
    expect(state.requests.map(({ id }) => id)).toEqual([1, 3]);
    expect(state.published).toEqual([]);
    expect(state.refreshes).toBe(0);
    state.requests[1].resolve(summary(3));
    await flushResponses();
    expect(state.published).toEqual([summary(3)]);
    expect(state.refreshes).toBe(1);
  });

  it.each([0, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1])(
    "refreshes invalid notification id %s without issuing a read",
    async (id) => {
      const state = controlledReader();
      state.reader.capture(id);
      await flushResponses();
      expect(state.requests).toEqual([]);
      expect(state.published).toEqual([]);
      expect(state.refreshes).toBe(1);
    },
  );

  it("invalid metadata invalidates pending work so a late success cannot overwrite its snapshot", async () => {
    const state = controlledReader();
    state.reader.capture(1);
    state.reader.capture(2);
    state.reader.capture(0);
    expect(state.refreshes).toBe(1);
    state.requests[0].resolve(summary(1));
    await flushResponses();
    expect(state.requests.map(({ id }) => id)).toEqual([1]);
    expect(state.published).toEqual([]);
    expect(state.refreshes).toBe(1);
  });

  it.each(["resolve", "reject"] as const)(
    "disposal drops queued work and ignores a late %s without refreshing or accepting new events",
    async (completion) => {
      const state = controlledReader();
      state.reader.capture(1);
      state.reader.capture(2);
      state.reader.dispose();
      state.reader.dispose();
      state.reader.capture(3);
      state.reader.capture(0);
      if (completion === "resolve") state.requests[0].resolve(summary(1));
      else state.requests[0].reject(new Error("disposed read failed late"));
      await flushResponses();
      expect(state.requests.map(({ id }) => id)).toEqual([1]);
      expect(state.published).toEqual([]);
      expect(state.refreshes).toBe(0);
    },
  );
});
