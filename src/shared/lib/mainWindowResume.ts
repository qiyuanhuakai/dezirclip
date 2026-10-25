export interface ResumeAuthority { instanceId: string; epoch: number }
export interface ResumeAccess { epoch: number; isCurrent: () => boolean; release?: () => void }
export interface PreparedResume { requestId: number | null; authority: ResumeAuthority; isCurrent: () => boolean; isRequestCurrent: () => boolean }
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;

export function readResumeAuthority(value: unknown): ResumeAuthority {
  if (!record(value) || !record(value.state) || !record(value.privacy)
    || typeof value.state.service_instance_id !== "string" || !value.state.service_instance_id
    || !Number.isSafeInteger(value.privacy.privacy_epoch) || Number(value.privacy.privacy_epoch) < 0
    || value.state.privacy_epoch !== value.privacy.privacy_epoch) throw new Error("ClipboardHostUnavailable");
  return { instanceId: value.state.service_instance_id, epoch: Number(value.privacy.privacy_epoch) };
}

export function createMainWindowResume(deps: {
  prepare: (current: () => boolean) => Promise<ResumeAccess>;
  readAuthority: () => Promise<unknown>;
  loadHistory: () => Promise<boolean>;
  pending: (requestId: number | null) => void;
  prepared: (value: PreparedResume) => void;
  failed: (requestId: number | null) => void;
}) {
  let revision = 0;
  let active: { id: number | null; access?: ResumeAccess } | null = null;
  let latest = 0;
  const cancel = () => { revision += 1; active?.access?.release?.(); active = null; };
  const run = async (requestId: number | null) => {
      active?.access?.release?.();
      const owner: { id: number | null; access?: ResumeAccess } = { id: requestId };
      active = owner;
      const request = ++revision;
      const current = () => revision === request && active === owner;
      deps.pending(requestId);
      try {
        const access = await deps.prepare(current);
        owner.access = access;
        if (!current()) { access.release?.(); return; }
        const authorized = () => current() && access.isCurrent();
        const before = readResumeAuthority(await deps.readAuthority());
        if (!authorized() || before.epoch !== access.epoch) throw new Error("PrivacyEpochChanged");
        if (!(await deps.loadHistory())) throw new Error("HistoryResumeFailed");
        const after = readResumeAuthority(await deps.readAuthority());
        if (!authorized() || after.epoch !== before.epoch || after.instanceId !== before.instanceId) throw new Error("ClipboardHostInstanceChanged");
        deps.prepared({ requestId, authority: after, isCurrent: authorized, isRequestCurrent: current });
      } catch {
        owner.access?.release?.();
        if (current()) { active = null; deps.failed(requestId); }
      }
  };
  return {
    cancel,
    async start(requestId: number) {
      if (!Number.isSafeInteger(requestId) || requestId <= 0 || requestId < latest || active?.id === requestId) return;
      latest = requestId;
      await run(requestId);
    },
    restoreVisible: () => run(null)
  };
}
