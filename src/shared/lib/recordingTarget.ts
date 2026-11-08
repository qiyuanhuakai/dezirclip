export type RecordingTarget = "main" | "sequential" | "rich" | "search";

/** Existing boolean setters expose one mutually exclusive recording target. */
export function updateRecordingTarget(
  current: RecordingTarget | null,
  target: RecordingTarget,
  update: boolean | ((recording: boolean) => boolean),
): RecordingTarget | null {
  const enabled = typeof update === "function" ? update(current === target) : update;
  return enabled ? target : current === target ? null : current;
}
