import { expect, it } from "vitest";
import { completeRecording } from "./recordingCompletion";

for (const reason of ["cancel", "unmount", "owner replacement"]) {
  it(`discards a delayed native stop after ${reason}`, async () => {
    let finish = () => {};
    const stopped = new Promise<void>(resolve => { finish = resolve; });
    const session = new AbortController();
    let current = true;
    const results: string[] = [];
    const pending = completeRecording(stopped, session.signal, () => current,
      () => results.push("saved"), () => results.push("failed"));
    if (reason === "owner replacement") current = false;
    else session.abort();
    finish();
    await pending;
    expect(results).toEqual([]);
  });
}

it("delivers success or failure only to a live session", async () => {
  const session = new AbortController();
  const results: unknown[] = [];
  const failed = (error: unknown) => { results.push(error); };
  await completeRecording(Promise.resolve(), session.signal, () => true,
    () => results.push("saved"), failed);
  await completeRecording(Promise.reject("stop failed"), session.signal, () => true,
    () => results.push("unexpected"), failed);
  session.abort();
  await completeRecording(Promise.reject("stale error"), session.signal, () => true,
    () => results.push("unexpected"), failed);
  expect(results).toEqual(["saved", "stop failed"]);
});
