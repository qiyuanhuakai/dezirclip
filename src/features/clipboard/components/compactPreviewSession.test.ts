import { describe, expect, it } from "vitest";
import { createPreviewSessionQueue } from "./compactPreviewSession";

function deferred() {
    let resolve = () => {};
    const promise = new Promise<void>(done => { resolve = done; });
    return { promise, resolve };
}

describe("preview request lifecycle", () => {
    it("does not show when the pointer leaves during positioning", async () => {
        const queue = createPreviewSessionQueue<string>();
        const position = deferred();
        const entered = deferred();
        const calls: string[] = [];
        const id = queue.begin({}, "record", () => true);
        const presenting = queue.run(id, [
            async () => { calls.push("position"); entered.resolve(); await position.promise; },
            async () => { calls.push("show"); }
        ]);
        await entered.promise;
        const hiding = queue.cancel(async () => { calls.push("hide"); });
        position.resolve();
        await Promise.all([presenting, hiding]);
        expect(calls).toEqual(["position", "hide"]);
    });

    it("always hides after an already submitted show completes", async () => {
        const queue = createPreviewSessionQueue<string>();
        const shown = deferred();
        const entered = deferred();
        let visible = false;
        const id = queue.begin({}, "record", () => true);
        const presenting = queue.run(id, [async () => {
            entered.resolve(); await shown.promise; visible = true;
        }]);
        await entered.promise;
        const hiding = queue.cancel(async () => { visible = false; });
        shown.resolve();
        await Promise.all([presenting, hiding]);
        expect(visible).toBe(false);
    });

    it("ignores a previous row's delayed layout, even for identical dimensions", async () => {
        const queue = createPreviewSessionQueue<string>();
        const first = queue.begin({}, "first", () => true);
        const second = queue.begin({}, "second", () => true);
        const shown: string[] = [];
        await queue.run(first, [async value => { shown.push(value); }]);
        await queue.run(second, [async value => { shown.push(value); }]);
        expect(shown).toEqual(["second"]);
    });

    it("an unrelated virtual row unmount cannot close the hovered row", async () => {
        const queue = createPreviewSessionQueue<string>();
        const owner = {};
        const id = queue.begin(owner, "hovered", () => true);
        let hidden = false;
        await queue.cancel(async () => { hidden = true; }, {});
        expect(queue.get(id)).toBe("hovered");
        expect(hidden).toBe(false);
        await queue.cancel(async () => { hidden = true; }, owner);
        expect(queue.get(id)).toBeNull();
        expect(hidden).toBe(true);
    });

    it("rechecks the privacy grant between every native operation", async () => {
        const queue = createPreviewSessionQueue<string>();
        let authorized = true;
        const id = queue.begin({}, "protected", () => authorized);
        const calls: string[] = [];
        await queue.run(id, [
            async () => { calls.push("size"); authorized = false; },
            async () => { calls.push("show"); }
        ]);
        expect(calls).toEqual(["size"]);
        expect(queue.current()).toBeNull();
    });

    it("keeps ownership available after leaving invalidates the hover grant", async () => {
        const queue = createPreviewSessionQueue<string>();
        const owner = {};
        let hovered = true;
        queue.begin(owner, "record", () => hovered);
        hovered = false;
        expect(queue.current()).toBeNull();
        expect(queue.owns(owner)).toBe(true);
        await queue.cancel(async () => {}, owner);
        expect(queue.owns(owner)).toBe(false);
    });

    it("a failed native operation does not block a later hide or a new request", async () => {
        const queue = createPreviewSessionQueue<string>();
        const first = queue.begin({}, "first", () => true);
        await expect(queue.run(first, [async () => { throw new Error("native failure"); }])).rejects.toThrow("native failure");
        const calls: string[] = [];
        await queue.cancel(async () => { calls.push("hide"); });
        const second = queue.begin({}, "second", () => true);
        await queue.run(second, [async value => { calls.push(value); }]);
        expect(calls).toEqual(["hide", "second"]);
    });
});
