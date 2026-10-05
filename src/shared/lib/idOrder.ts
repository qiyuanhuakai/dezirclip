/**
 * Comparing two id orderings for equality.
 *
 * Pinned rows are stored as their id sequence and the effect that syncs them
 * runs whenever the pinned list is rebuilt. Rebuilding the list produces a new
 * array even when the order in it is identical, and handing that new array to a
 * state setter is a state change whether or not the value differs -- so an
 * ordering that has not moved costs a render pass. The comparison is the
 * cheapest way to tell the two cases apart, and it is a pure function so the
 * rule can be pinned down without a DOM.
 */
export const sameIdOrder = (a: readonly number[], b: readonly number[]): boolean => {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
};
