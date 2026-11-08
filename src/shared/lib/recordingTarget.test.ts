import { expect, it } from "vitest";
import { updateRecordingTarget, type RecordingTarget } from "./recordingTarget";
import { completeRecording } from "./recordingCompletion";

it("switches the active recorder instead of retaining two boolean flags", () => {
  let target = updateRecordingTarget(null, "rich", true);
  target = updateRecordingTarget(target, "search", true);
  expect(target).toBe("search");
  expect(updateRecordingTarget(target, "rich", false)).toBe("search");
  expect(updateRecordingTarget(target, "search", false)).toBe(null);
  expect(updateRecordingTarget("main", "sequential", previous => !previous)).toBe("sequential");
});

for (const cancelSearch of [false, true]) {
  it(`discards the old recorder completion after switching${cancelSearch ? " and cancelling" : ""}`, async () => {
    let target: RecordingTarget | null = "rich";
    let finish = () => {};
    const stop = new Promise<void>(resolve => { finish = resolve; });
    const results: string[] = [];
    const complete = (captured: RecordingTarget) => completeRecording(stop, new AbortController().signal,
      () => target === captured, () => { results.push(captured); }, () => { results.push("error"); });
    const rich = complete("rich");
    target = updateRecordingTarget(target, "search", true);
    const search = complete("search");
    if (cancelSearch) target = updateRecordingTarget(target, "search", false);
    finish();
    await Promise.all([rich, search]);
    expect(results).toEqual(cancelSearch ? [] : ["search"]);
  });
}
