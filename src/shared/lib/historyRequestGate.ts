/** A refresh may supersede an older read; pagination may never supersede a refresh. */
export function createHistoryRequestGate() {
  let active: object | null = null;
  let appliedKey: string | null = null;
  return {
    canAppend: (key: string) => active === null && appliedKey === key,
    hasApplied: (key: string) => appliedKey === key,
    invalidate() { active = null; appliedKey = null; },
    begin(key: string, reset: boolean) {
      if (!reset && (active !== null || appliedKey !== key)) return null;
      const owner = {};
      active = owner;
      return {
        isCurrent: (currentKey: string) => active === owner && currentKey === key,
        finish(success: boolean, currentKey: string) {
          if (active !== owner) return;
          active = null;
          if (success && currentKey === key) appliedKey = key;
          else if (reset) appliedKey = null;
        },
      };
    },
  };
}
