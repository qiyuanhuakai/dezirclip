import {
  applyQueueEvent,
  applyQueueRead,
  INITIAL_PASTE_QUEUE_COUNT_STATE,
  readToken,
  type PasteQueueCountState,
} from "./pasteQueueCount";

describe("pasteQueueCount", () => {
  // The interleaving that matters, spelled out as an ordered script so a
  // failure names the step that broke rather than just "expected 1, got 0".
  describe("a read that an event overtook", () => {
    let state: PasteQueueCountState;
    let emptyReadToken: number;

    beforeEach(() => {
      state = INITIAL_PASTE_QUEUE_COUNT_STATE;
      // Queue is empty; focus triggers a read and the backend samples [].
      emptyReadToken = readToken(state);
      // A capture enqueues one item and its event arrives before the read.
      state = applyQueueEvent(state, 1);
    });

    it("does not let the overtaken read put the count back", () => {
      const landed = applyQueueRead(state, emptyReadToken, 0);
      expect(landed.count).toBe(1);
    });

    it("does not let the overtaken read retire the revision either", () => {
      const landed = applyQueueRead(state, emptyReadToken, 0);
      // If the stale read were allowed to rewrite the state it would hand back
      // a revision the next read would then happily match against.
      expect(landed.revision).toBe(state.revision);
    });
  });

  it("still lands a read that nothing overtook", () => {
    const token = readToken(INITIAL_PASTE_QUEUE_COUNT_STATE);
    const landed = applyQueueRead(INITIAL_PASTE_QUEUE_COUNT_STATE, token, 3);
    expect(landed.count).toBe(3);
  });

  // Without this the guard above would "pass" by discarding every read.
  it("a read that lands first is not discarded by a later event being absent", () => {
    const token = readToken(INITIAL_PASTE_QUEUE_COUNT_STATE);
    const landed = applyQueueRead(INITIAL_PASTE_QUEUE_COUNT_STATE, token, 2);
    expect(applyQueueRead(landed, readToken(landed), 5).count).toBe(5);
  });

  it("treats a failed read like any other: stale ones are dropped, current ones land as 0", () => {
    const token = readToken(INITIAL_PASTE_QUEUE_COUNT_STATE);
    expect(applyQueueRead(INITIAL_PASTE_QUEUE_COUNT_STATE, token, 0).count).toBe(0);

    const afterEvent = applyQueueEvent(INITIAL_PASTE_QUEUE_COUNT_STATE, 1);
    expect(applyQueueRead(afterEvent, token, 0).count).toBe(1);
  });

  it("lets a read issued after the event land, since it sampled after it", () => {
    const afterEvent = applyQueueEvent(INITIAL_PASTE_QUEUE_COUNT_STATE, 1);
    const freshToken = readToken(afterEvent);
    expect(applyQueueRead(afterEvent, freshToken, 1).count).toBe(1);
  });

  it("only advances the revision for events, so two reads can race harmlessly", () => {
    const first = applyQueueRead(INITIAL_PASTE_QUEUE_COUNT_STATE, 0, 4);
    expect(first.revision).toBe(0);
    const second = applyQueueRead(first, 0, 4);
    expect(second.revision).toBe(0);
    expect(applyQueueEvent(second, 9).revision).toBe(1);
  });
});
