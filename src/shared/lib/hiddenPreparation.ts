export function createHiddenPreparationGate() {
  let active: { mode: "main" | "preview"; current: () => boolean } | null = null;
  const current = () => {
    if (active && !active.current()) active = null;
    return active;
  };
  return {
    begin(mode: "main" | "preview", valid: () => boolean) {
      const owner = { mode, current: valid };
      active = owner;
      return {
        isCurrent: () => current() === owner,
        release: () => { if (active === owner) active = null; }
      };
    },
    current,
    revoke: () => { active = null; }
  };
}
