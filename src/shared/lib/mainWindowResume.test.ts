import { describe, expect, it } from "vitest";
import { createMainWindowResume, readResumeAuthority, type PreparedResume } from "./mainWindowResume";

const status = (instance = "host-a", epoch = 4) => ({ state: { service_instance_id: instance, privacy_epoch: epoch }, privacy: { privacy_epoch: epoch } });
function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (error: Error) => void = () => {};
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function fixture() {
  const calls: string[] = [];
  const prepared: PreparedResume[] = [];
  const failed: Array<number | null> = [];
  const deps = {
    prepare: async (_current: () => boolean) => { calls.push("privacy"); return { epoch: 4, isCurrent: () => true }; },
    readAuthority: async (): Promise<unknown> => { calls.push("authority"); return status(); },
    loadHistory: async () => { calls.push("history"); return true; },
    pending: (id: number | null) => { calls.push(`pending:${id}`); },
    prepared: (value: PreparedResume) => { calls.push("prepared"); prepared.push(value); },
    failed: (id: number | null) => { failed.push(id); }
  };
  return { calls, prepared, failed, deps };
}
describe("main window resume", () => {
  it("an already-visible recovery has no native presentation request to acknowledge", async () => {
    const f = fixture(); await createMainWindowResume(f.deps).restoreVisible();
    expect(f.prepared[0].requestId).toBeNull();
    expect(f.calls).toEqual(["pending:null", "privacy", "authority", "history", "authority", "prepared"]);
  });
  it("requires a complete owner identity and matching status epochs", () => {
    expect(readResumeAuthority(status())).toEqual({ instanceId: "host-a", epoch: 4 });
    for (const value of [null, {}, status(""), status("a", -1), { ...status(), privacy: { privacy_epoch: 5 } }]) {
      expect(() => readResumeAuthority(value)).toThrow();
    }
  });
  it("prepares privacy, checks ownership, loads the page and rechecks ownership before presentation", async () => {
    const f = fixture(); const controller = createMainWindowResume(f.deps);
    await controller.start(1);
    expect(f.calls).toEqual(["pending:1", "privacy", "authority", "history", "authority", "prepared"]);
    expect(f.prepared[0].isCurrent()).toBe(true);
  });
  it("cannot report prepared while the history page is still pending", async () => {
    const f = fixture(); const history = deferred<boolean>(); f.deps.loadHistory = () => history.promise;
    const task = createMainWindowResume(f.deps).start(1);
    await Promise.resolve(); await Promise.resolve();
    expect(f.prepared).toEqual([]);
    history.resolve(true); await task;
    expect(f.prepared).toHaveLength(1);
  });
  it("does not read history after a status/owner epoch mismatch", async () => {
    const f = fixture(); f.deps.readAuthority = async () => status("host-a", 5);
    await createMainWindowResume(f.deps).start(1);
    expect(f.calls).not.toContain("history"); expect(f.failed).toEqual([1]);
  });
  it("keeps failed history reads distinct from a valid empty database", async () => {
    const f = fixture(); f.deps.loadHistory = async () => false;
    await createMainWindowResume(f.deps).start(1);
    expect(f.prepared).toEqual([]); expect(f.failed).toEqual([1]);
  });
  it("rejects an owner switch during the page read", async () => {
    const f = fixture(); let reads = 0;
    f.deps.readAuthority = async () => status(++reads === 1 ? "host-a" : "host-b");
    await createMainWindowResume(f.deps).start(1);
    expect(f.prepared).toEqual([]); expect(f.failed).toEqual([1]);
  });
  it("rejects an epoch change during the page read", async () => {
    const f = fixture(); let reads = 0;
    f.deps.readAuthority = async () => status("host-a", ++reads === 1 ? 4 : 5);
    await createMainWindowResume(f.deps).start(1);
    expect(f.prepared).toEqual([]); expect(f.failed).toEqual([1]);
  });
  it("keeps the privacy fence live through the DOM commit", async () => {
    const f = fixture(); let current = true;
    f.deps.prepare = async () => ({ epoch: 4, isCurrent: () => current });
    await createMainWindowResume(f.deps).start(1);
    current = false; expect(f.prepared[0].isCurrent()).toBe(false);
  });
  it("drops a cancelled page without presenting or failing a newer request", async () => {
    const f = fixture(); const old = deferred<boolean>(); let loads = 0;
    f.deps.loadHistory = () => ++loads === 1 ? old.promise : Promise.resolve(true);
    const controller = createMainWindowResume(f.deps);
    const first = controller.start(1); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    controller.cancel(); await controller.start(2); old.resolve(true); await first;
    expect(f.prepared.map(value => value.requestId)).toEqual([2]); expect(f.failed).toEqual([]);
  });
  it("deduplicates same-request events and ignores late older request IDs", async () => {
    const f = fixture(); const controller = createMainWindowResume(f.deps);
    await controller.start(2); await controller.start(2); await controller.start(1);
    expect(f.prepared).toHaveLength(1);
  });
  it("allows an explicit retry after failure without requiring a new native show", async () => {
    const f = fixture(); f.deps.loadHistory = async () => false;
    const controller = createMainWindowResume(f.deps); await controller.start(1);
    f.deps.loadHistory = async () => true; await controller.start(1);
    expect(f.prepared.map(value => value.requestId)).toEqual([1]);
  });
  it("does not let an old rejected request clear the new prepared page", async () => {
    const f = fixture(); const old = deferred<unknown>(); let reads = 0;
    f.deps.readAuthority = () => ++reads === 1 ? old.promise : Promise.resolve(status());
    const controller = createMainWindowResume(f.deps); const first = controller.start(1);
    await Promise.resolve(); await controller.start(2); old.reject(new Error("old failure")); await first;
    expect(f.prepared.map(value => value.requestId)).toEqual([2]); expect(f.failed).toEqual([]);
  });
});
