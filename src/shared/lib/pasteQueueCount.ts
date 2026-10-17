/**
 * Count tracking for the sequential-paste queue, as a pure state machine.
 *
 * The count has two sources and they do not arrive in order. A read
 * (`get_paste_queue`) is an IPC round trip that an event
 * (`paste-queue-changed`) can overtake, because that event already carries the
 * newer length. A read issued before an event must not land on top of it, or
 * this sequence silently hides a queued batch:
 *
 *   1. the queue is empty and a window focus triggers a read; the backend
 *      samples `[]` and the response is still in flight
 *   2. a capture enqueues one item and `paste-queue-changed(1)` arrives first,
 *      so the count correctly becomes 1
 *   3. the in-flight read's `[]` lands and puts the count back to 0
 *   4. nothing else fires, so the count and the clear button stay hidden
 *
 * The asymmetry is deliberate: an event is pushed by the very mutation that
 * changed the queue and carries its result, whereas a read only samples
 * whatever was true when the backend got round to answering. So an event
 * retires every read that was already in flight.
 */
export interface PasteQueueCountState {
  /** What the UI shows. */
  readonly count: number;
  /** Advanced by every authoritative push; a read carries the value it saw. */
  readonly revision: number;
}

export const INITIAL_PASTE_QUEUE_COUNT_STATE: PasteQueueCountState = {
  count: 0,
  revision: 0,
};

/**
 * Capture what a read has to still match to be allowed to land.
 *
 * Take this when the read is issued, not when it resolves — that gap is the
 * whole bug.
 */
export const readToken = (state: PasteQueueCountState): number => state.revision;

/**
 * Land a read issued at `token`.
 *
 * A stale read changes nothing: it is not evidence about the present, and
 * overwriting with it is how a queued batch disappears from the UI.
 */
export const applyQueueRead = (
  state: PasteQueueCountState,
  token: number,
  next: number
): PasteQueueCountState =>
  // The revision has to survive the write. Dropping it would leave the next
  // read holding `undefined` and matching nothing, so a second refresh would
  // be discarded for ever after the first one landed.
  token === state.revision ? { count: next, revision: state.revision } : state;

/** Land a length the backend pushed, retiring every read issued before it. */
export const applyQueueEvent = (
  state: PasteQueueCountState,
  next: number
): PasteQueueCountState => ({ count: next, revision: state.revision + 1 });
