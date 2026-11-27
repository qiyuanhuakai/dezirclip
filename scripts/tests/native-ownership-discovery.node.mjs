import assert from "node:assert/strict";
import test from "node:test";
import { discoverOwnershipPage, ownershipDiscoveryBudget, ownershipDiscoveryPolicy } from "./native-ownership-discovery.mjs";

const target = { type: "page", url: "http://tauri.localhost/", webSocketDebuggerUrl: "ws://127.0.0.1:9222/synthetic" };

test("discovery caps requests and pauses using the remaining monotonic budget", () => {
  assert.deepEqual(ownershipDiscoveryPolicy, { maximumAttempts: 180, requestTimeoutMs: 1500, pauseMs: 250, totalBudgetMs: 315000 });
  assert.equal(ownershipDiscoveryBudget(10, 10, 0).active, true);
  assert.equal(ownershipDiscoveryBudget(10, 315010, 0).active, false);
  assert.equal(ownershipDiscoveryBudget(10, 11, 180).active, false);
  const budget = ownershipDiscoveryBudget(10, 314910, 1);
  assert.equal(budget.requestTimeoutMs, 100);
  assert.equal(budget.pauseMs, 100);
  assert.throws(() => ownershipDiscoveryBudget(10, 9, 1));
  assert.throws(() => ownershipDiscoveryBudget(10, 10, -1));
});

const scenarios = [
  { mode: "early", expectedCalls: 1, expectedPauses: 0, elapsed: 1, stage: "http-main-page" },
  { mode: "late", expectedCalls: 1, expectedPauses: 0, elapsed: 315000, stage: "main-page-after-deadline" },
  { mode: "refused", expectedCalls: 180, expectedPauses: 179, elapsed: 44750, stage: "tcp-refused" },
  { mode: "slow", expectedCalls: 180, expectedPauses: 179, elapsed: 314750, stage: "http-without-main-page" },
  { mode: "abort", expectedCalls: 180, expectedPauses: 179, elapsed: 314750, stage: "request-aborted" },
  { mode: "invalid", expectedCalls: 180, expectedPauses: 179, elapsed: 314750, stage: "request-or-body-failed" },
];

for (const { mode, expectedCalls, expectedPauses, elapsed, stage } of scenarios) {
  test(`actual bounded discovery records ${mode} starting from synthetic input`, async () => {
    const evidence = { observations: [] };
    const requestBudgets = [];
    const pauses = [];
    let now = 0;
    let calls = 0;
    const options = {
      now: () => now,
      requestSignal(milliseconds) {
        requestBudgets.push(milliseconds);
        return "synthetic-timeout";
      },
      async request(url, { signal }) {
        assert.equal(url, "http://127.0.0.1:9222/json/list");
        assert.equal(signal, "synthetic-timeout");
        calls++;
        if (mode === "refused") throw new Error("Cannot connect", { cause: { code: "ECONNREFUSED" } });
        now += mode === "late" ? 315000 : mode === "early" ? 1 : 1500;
        if (mode === "abort") throw new DOMException("Synthetic timeout", "TimeoutError");
        return { status: 200, json: async () => mode === "invalid" ? {} : mode === "early" || mode === "late" ? [target] : [] };
      },
      async delay(milliseconds) {
        pauses.push(milliseconds);
        now += milliseconds;
      },
    };
    const result = discoverOwnershipPage(9222, evidence, options);
    if (mode === "early") assert.equal(await result, target);
    else await assert.rejects(result, /did not expose its main WebView/);
    assert.equal(calls, expectedCalls);
    assert.equal(pauses.length, expectedPauses);
    assert.equal(evidence.discovery.elapsed_ms, elapsed);
    assert.equal(evidence.discovery.attempts, calls);
    assert(evidence.observations.every(row => row.stage === stage));
    assert(requestBudgets.every(milliseconds => milliseconds > 0 && milliseconds <= 1500));
    assert(pauses.every(milliseconds => milliseconds > 0 && milliseconds <= 250));
  });
}
