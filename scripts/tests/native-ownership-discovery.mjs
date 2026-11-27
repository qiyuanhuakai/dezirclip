import assert from "node:assert/strict";

export const ownershipDiscoveryPolicy = Object.freeze({ maximumAttempts: 180, requestTimeoutMs: 1500, pauseMs: 250, totalBudgetMs: 180 * (1500 + 250) });
export function ownershipDiscoveryBudget(started, now, attempt) {
  assert(Number.isFinite(started) && Number.isFinite(now) && now >= started, "Discovery clock must advance monotonically");
  assert(Number.isSafeInteger(attempt) && attempt >= 0, "Invalid discovery attempt");
  const remainingMs = Math.max(0, ownershipDiscoveryPolicy.totalBudgetMs - (now - started));
  return { active: attempt < ownershipDiscoveryPolicy.maximumAttempts && remainingMs >= 1,
    remainingMs, requestTimeoutMs: Math.floor(Math.min(ownershipDiscoveryPolicy.requestTimeoutMs, remainingMs)),
    pauseMs: Math.floor(Math.min(ownershipDiscoveryPolicy.pauseMs, remainingMs)) };
}
export function ownershipDiscoveryStage(error, status, mainPage) {
  if (error?.cause?.code === "ECONNREFUSED") return "tcp-refused";
  if (error?.name === "TimeoutError" || error?.name === "AbortError") return "request-aborted";
  if (error) return "request-or-body-failed";
  return mainPage ? "http-main-page" : "http-without-main-page";
}

export async function discoverOwnershipPage(port, evidence, {
  now = () => performance.now(),
  request = fetch,
  requestSignal = milliseconds => AbortSignal.timeout(milliseconds),
  delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds)),
} = {}) {
  let page;
  const discoveryStarted = now();
  evidence.discovery = { policy: ownershipDiscoveryPolicy, started_monotonic_ms: discoveryStarted,
    scope: "Existing bounded CDP requests only; no process query, installation or GUI launch is repeated." };
  for (let attempt = 0; attempt < ownershipDiscoveryPolicy.maximumAttempts; attempt++) {
    const budget = ownershipDiscoveryBudget(discoveryStarted, now(), attempt);
    if (!budget.active) break;
    const observation = { attempt, started_monotonic_ms: now(), request_timeout_ms: budget.requestTimeoutMs };
    try {
      const response = await request(`http://127.0.0.1:${port}/json/list`, { signal: requestSignal(budget.requestTimeoutMs) });
      observation.status = response.status;
      const targets = await response.json(); observation.targets = targets;
      assert.equal(response.status, 200, "Fixture CDP discovery did not return HTTP 200");
      assert(Array.isArray(targets), "Fixture CDP discovery did not return a target list");
      const candidate = targets.find(target => target.type === "page" && target.webSocketDebuggerUrl && /^(?:https?:\/\/tauri\.localhost(?:[:/]|$)|tauri:\/\/)/.test(target.url) && !/[?&]window=/.test(target.url));
      observation.stage = ownershipDiscoveryStage(null, response.status, Boolean(candidate));
      const completed = now();
      if (candidate && ownershipDiscoveryBudget(discoveryStarted, completed, attempt).active) page = candidate;
      else if (candidate) observation.stage = "main-page-after-deadline";
    } catch (error) {
      observation.error = String(error); observation.cause = String(error.cause ?? "");
      observation.stage = ownershipDiscoveryStage(error, observation.status, false);
    } finally {
      observation.completed_monotonic_ms = now(); evidence.observations.push(observation);
    }
    if (page) break;
    const rest = ownershipDiscoveryBudget(discoveryStarted, now(), attempt + 1);
    if (!rest.active) break;
    await delay(rest.pauseMs);
  }
  evidence.discovery.completed_monotonic_ms = now();
  evidence.discovery.elapsed_ms = evidence.discovery.completed_monotonic_ms - discoveryStarted;
  evidence.discovery.attempts = evidence.observations.length;
  if (!page) throw new Error("Fixture application did not expose its main WebView");
  return page;
}
