/**
 * Keyboard selection movement for the clipboard list.
 *
 * The step is computed by the caller from the index React hands back, not from a
 * captured value, so two presses landing in the same tick still advance twice.
 * It lives outside the keydown hook because the clamp is the part worth pinning
 * down: the list is the only thing that knows how long it is, and an unclamped
 * step parks the selection on an index that renders nothing.
 */

export type ArrowKey = "ArrowDown" | "ArrowUp";

export const arrowSelectionStep = (key: ArrowKey, current: number, total: number): number => {
  if (key === "ArrowDown") {
    return Math.min(current + 1, total - 1);
  }
  return Math.max(current - 1, 0);
};
