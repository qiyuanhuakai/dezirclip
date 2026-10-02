import { describe, expect, it, vi } from "vitest";
import { createScrollCoalescer } from "./scrollCoalesce";

type Frame = () => void;

const createHarness = () => {
    // Mirrors requestAnimationFrame/cancelAnimationFrame: a cancelled handle is
    // removed from the queue, so a stale frame can never run.
    const queued = new Map<number, Frame>();
    const cancelled: number[] = [];
    let nextHandle = 1;
    let top = 0;

    const requestFrame = (callback: Frame) => {
        const id = nextHandle++;
        queued.set(id, callback);
        return id;
    };
    const cancelFrame = (handle: number) => {
        cancelled.push(handle);
        queued.delete(handle);
    };
    const readOffset = () => top;

    return {
        queuedCount: () => queued.size,
        cancelled,
        setTop: (value: number) => {
            top = value;
        },
        runAll: () => {
            const batch = [...queued.values()];
            queued.clear();
            batch.forEach((frame) => frame());
            return batch.length;
        },
        options: { requestFrame, cancelFrame, readOffset }
    };
};

const createElement = () => document.createElement("div");

const fireScroll = (element: HTMLElement) => {
    element.dispatchEvent(new Event("scroll"));
};

describe("createScrollCoalescer", () => {
    it("reports the offset synchronously for every event", () => {
        const harness = createHarness();
        const onScroll = vi.fn();
        const element = createElement();
        const coalescer = createScrollCoalescer(onScroll, vi.fn(), harness.options);
        coalescer.attach(element);

        harness.setTop(10);
        fireScroll(element);
        harness.setTop(20);
        fireScroll(element);

        expect(onScroll.mock.calls).toEqual([[10], [20]]);
    });

    it("collapses a burst of events into a single frame carrying the latest offset", () => {
        const harness = createHarness();
        const onScroll = vi.fn();
        const onFrame = vi.fn();
        const element = createElement();
        const coalescer = createScrollCoalescer(onScroll, onFrame, harness.options);
        coalescer.attach(element);

        for (let i = 1; i <= 50; i += 1) {
            harness.setTop(i);
            fireScroll(element);
        }

        expect(onFrame).not.toHaveBeenCalled();
        expect(harness.queuedCount()).toBe(1);

        harness.runAll();

        expect(onScroll).toHaveBeenCalledTimes(50);
        expect(onFrame).toHaveBeenCalledTimes(1);
        expect(onFrame).toHaveBeenCalledWith(50);
    });

    it("tracks whether a frame is pending", () => {
        const harness = createHarness();
        const element = createElement();
        const coalescer = createScrollCoalescer(vi.fn(), vi.fn(), harness.options);
        coalescer.attach(element);

        expect(coalescer.pending).toBe(false);
        fireScroll(element);
        expect(coalescer.pending).toBe(true);
        harness.runAll();
        expect(coalescer.pending).toBe(false);
    });

    it("seeds the offset on attach without emitting", () => {
        const harness = createHarness();
        harness.setTop(640);
        const onScroll = vi.fn();
        const onFrame = vi.fn();
        const element = createElement();
        const coalescer = createScrollCoalescer(onScroll, onFrame, harness.options);

        coalescer.attach(element);

        expect(onScroll).not.toHaveBeenCalled();
        expect(onFrame).not.toHaveBeenCalled();
        expect(coalescer.pending).toBe(false);

        fireScroll(element);
        expect(onScroll).toHaveBeenCalledWith(640);
    });

    it("flushes a pending frame immediately and reports it ran", () => {
        const harness = createHarness();
        const onFrame = vi.fn();
        const element = createElement();
        const coalescer = createScrollCoalescer(vi.fn(), onFrame, harness.options);
        coalescer.attach(element);

        harness.setTop(300);
        fireScroll(element);

        expect(coalescer.flush()).toBe(true);
        expect(onFrame).toHaveBeenCalledWith(300);
        expect(coalescer.pending).toBe(false);
        expect(harness.cancelled).toHaveLength(1);

        expect(coalescer.flush()).toBe(false);
        expect(onFrame).toHaveBeenCalledTimes(1);
    });

    it("does not run a flushed frame again when the scheduler catches up", () => {
        const harness = createHarness();
        const onFrame = vi.fn();
        const element = createElement();
        const coalescer = createScrollCoalescer(vi.fn(), onFrame, harness.options);
        coalescer.attach(element);

        fireScroll(element);
        coalescer.flush();
        harness.runAll();

        expect(onFrame).toHaveBeenCalledTimes(1);
    });

    it("cancels the pending frame on detach", () => {
        const harness = createHarness();
        const onFrame = vi.fn();
        const element = createElement();
        const coalescer = createScrollCoalescer(vi.fn(), onFrame, harness.options);
        coalescer.attach(element);

        fireScroll(element);
        coalescer.detach();

        expect(harness.cancelled).toHaveLength(1);
        expect(coalescer.pending).toBe(false);

        harness.runAll();
        expect(onFrame).not.toHaveBeenCalled();
    });

    it("stops reporting events after detach", () => {
        const harness = createHarness();
        const onScroll = vi.fn();
        const element = createElement();
        const coalescer = createScrollCoalescer(onScroll, vi.fn(), harness.options);
        coalescer.attach(element);

        fireScroll(element);
        coalescer.detach();
        fireScroll(element);

        expect(onScroll).toHaveBeenCalledTimes(1);
    });

    it("rebinds to a new element and stops reporting the old one", () => {
        const harness = createHarness();
        const onScroll = vi.fn();
        const first = createElement();
        const second = createElement();
        const coalescer = createScrollCoalescer(onScroll, vi.fn(), harness.options);

        coalescer.attach(first);
        coalescer.attach(second);

        fireScroll(first);
        expect(onScroll).not.toHaveBeenCalled();

        fireScroll(second);
        expect(onScroll).toHaveBeenCalledTimes(1);
    });

    it("does not stack listeners when attached to the same element twice", () => {
        const harness = createHarness();
        const onScroll = vi.fn();
        const element = createElement();
        const coalescer = createScrollCoalescer(onScroll, vi.fn(), harness.options);

        coalescer.attach(element);
        coalescer.attach(element);
        fireScroll(element);

        expect(onScroll).toHaveBeenCalledTimes(1);
    });

    it("keeps accepting events when the scheduler runs the frame inline", () => {
        const onScroll = vi.fn();
        const onFrame = vi.fn();
        const element = createElement();
        let top = 0;
        const coalescer = createScrollCoalescer(onScroll, onFrame, {
            requestFrame: (callback) => {
                callback();
                return 1;
            },
            cancelFrame: () => undefined,
            readOffset: () => top
        });
        coalescer.attach(element);

        for (let i = 1; i <= 5; i += 1) {
            top = i;
            fireScroll(element);
        }

        expect(onScroll).toHaveBeenCalledTimes(5);
        expect(onFrame).toHaveBeenCalledTimes(5);
        expect(onFrame).toHaveBeenLastCalledWith(5);
        expect(coalescer.pending).toBe(false);
    });

    it("registers the listener as passive", () => {
        const harness = createHarness();
        const element = createElement();
        const addEventListener = vi.spyOn(element, "addEventListener");
        const coalescer = createScrollCoalescer(vi.fn(), vi.fn(), harness.options);

        coalescer.attach(element);

        expect(addEventListener).toHaveBeenCalledWith("scroll", expect.any(Function), {
            passive: true
        });
    });
});
