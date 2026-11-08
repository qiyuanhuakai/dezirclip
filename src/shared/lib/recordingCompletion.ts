/** Deliver a native stop result only to the recording session that requested it. */
export async function completeRecording(
  stopped: Promise<void>,
  signal: AbortSignal,
  isCurrent: () => boolean,
  completed: () => void,
  failed: (error: unknown) => void,
): Promise<void> {
  try {
    await stopped;
  } catch (error) {
    if (!signal.aborted && isCurrent()) failed(error);
    return;
  }
  if (!signal.aborted && isCurrent()) completed();
}
