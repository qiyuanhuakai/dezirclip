import { describe, expect, it, vi } from "vitest";
import { createLatestAppearanceSender } from "./latestAppearanceSender";

function gate() {
  let release = () => {};
  const promise = new Promise<void>(resolve => { release = resolve; });
  return { promise, release };
}

describe("latest native appearance sender", () => {
  it("sends only C after blocked A when B and C arrive", async () => {
    const first = gate();
    const send = vi.fn(async (value: string) => { if (value === "A") await first.promise; });
    const sender = createLatestAppearanceSender(send, vi.fn());
    sender.offer("A"); sender.offer("B"); sender.offer("C");
    expect(send.mock.calls).toEqual([["A"]]);
    first.release();
    await vi.waitFor(() => expect(send.mock.calls).toEqual([["A"], ["C"]]));
  });

  it("keeps a return to A while B is in flight", async () => {
    const blocked = gate();
    const calls: string[] = [];
    let applied = "A";
    const sender = createLatestAppearanceSender(async (value: string) => {
      if (value === applied) return;
      calls.push(value);
      if (value === "B") await blocked.promise;
      applied = value;
    }, vi.fn());
    sender.offer("B"); sender.offer("A");
    blocked.release();
    await vi.waitFor(() => expect(calls).toEqual(["B", "A"]));
    expect(applied).toBe("A");
  });

  it("continues with the latest snapshot after a failed send", async () => {
    const blocked = gate();
    const report = vi.fn();
    const send = vi.fn(async (value: string) => { if (value === "A") { await blocked.promise; throw new Error("disconnected"); } });
    const sender = createLatestAppearanceSender(send, report);
    sender.offer("A"); sender.offer("B"); sender.offer("C"); blocked.release();
    await vi.waitFor(() => expect(send.mock.calls).toEqual([["A"], ["C"]]));
    expect(report).toHaveBeenCalledOnce();
  });

  it("drops queued updates when its page closes", async () => {
    const blocked = gate();
    const send = vi.fn(async () => { await blocked.promise; });
    const sender = createLatestAppearanceSender<string>(send, vi.fn());
    sender.offer("A"); sender.offer("B"); sender.close(); sender.offer("C"); blocked.release();
    await blocked.promise;
    await Promise.resolve();
    expect(send).toHaveBeenCalledTimes(1);
  });
});
