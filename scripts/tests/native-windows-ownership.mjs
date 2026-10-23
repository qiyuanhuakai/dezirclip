import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { existsSync, lstatSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { waitForNativeProcess } from "./native-process-deadline.mjs";

// This bridge observes the installed GUI; it never mocks invoke or emits fixture events.
if (process.env.GITHUB_ACTIONS !== "true" || process.env.RUNNER_ENVIRONMENT !== "github-hosted" || process.platform !== "win32" || !process.env.RUNNER_TEMP) {
  throw new Error("Native ownership acceptance requires a disposable GitHub-hosted Windows runner");
}
const port = Number(process.argv[2]);
if (!Number.isSafeInteger(port) || port < 1024 || port > 65535) throw new Error("Invalid fixture CDP port");
function fixturePath(value) {
  assert.equal(typeof value, "string");
  const resolved = path.resolve(value);
  const relative = path.relative(path.resolve(process.env.RUNNER_TEMP), resolved);
  assert(relative && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative), "Fixture path must stay beneath RUNNER_TEMP");
  const boundary = path.resolve(process.env.RUNNER_TEMP);
  for (let current = resolved; ; current = path.dirname(current)) {
    if (existsSync(current)) assert(!lstatSync(current).isSymbolicLink(), "Fixture path cannot cross a symlink or junction");
    if (current.toLowerCase() === boundary.toLowerCase()) break;
  }
  return resolved;
}
const output = fixturePath(process.argv[3]);
const input = fixturePath(process.argv[4]);
const fixtureRoot = path.dirname(output);
assert.equal(path.dirname(input).toLowerCase(), fixtureRoot.toLowerCase(), "Input and evidence must use the same fixture");
assert.match(path.basename(fixtureRoot), /^dezirclip-upgrade-[a-f0-9]{32}$/);
const configuration = JSON.parse((await readFile(input, "utf8")).replace(/^\uFEFF/, ""));
assert.equal(fixturePath(configuration.profileDirectory).toLowerCase(), path.join(fixtureRoot, "profile").toLowerCase());
assert.match(configuration.expectedVersion, /^0\.2\.0-alpha\.[12]$/);
assert.match(configuration.marker, /^nativeupgradefixturemarker[a-f0-9]{32}$/);
assert.equal(typeof configuration.acquireOwnership, "boolean");
assert.equal(typeof configuration.verifyCapture, "boolean");
const evidence = { schemaVersion: 3, configuration, observations: [], nativeClipboardWrites: [], result: "running" };
const delay = (milliseconds) => new Promise(resolve => setTimeout(resolve, milliseconds));
let socket;
let page;
const pending = new Map();
let sequence = 0;
function command(method, params) {
  if (socket?.readyState !== WebSocket.OPEN) return Promise.reject(new Error("Fixture CDP socket is not open"));
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => { pending.delete(id); reject(new Error("Fixture CDP operation timed out")); }, 30000);
    pending.set(id, {
      resolve: value => { clearTimeout(timeout); resolve(value); },
      reject: error => { clearTimeout(timeout); reject(error); },
    });
    try { socket.send(JSON.stringify({ id, method, params })); }
    catch (error) { pending.delete(id); clearTimeout(timeout); reject(error); }
  });
}
async function evaluate(expression) {
  const result = await command("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (result?.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  return result?.result?.value;
}
const invoke = (command, args = {}) => evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`);
async function history() {
  const entries = await invoke("get_clipboard_history", { limit: 150, offset: 0, contentType: null });
  assert(Array.isArray(entries), "Actual GUI history did not return entries");
  return entries;
}
async function poll(label, operation) {
  const deadline = Date.now() + 20000;
  let lastError;
  do {
    try { const value = await operation(); if (value) return value; }
    catch (error) { lastError = error; }
    await delay(150);
  } while (Date.now() < deadline);
  throw new Error(`${label} timed out${lastError ? `: ${lastError}` : ""}`);
}
async function setNativeClipboard(text) {
  // Supply synthetic text as environment data, never shell source.
  const observation = { startedAt: new Date().toISOString(), status: "starting", stdout: "", stderr: "" };
  evidence.nativeClipboardWrites.push(observation);
  const child = spawn("python", [fileURLToPath(new URL("./native-windows-clipboard.py", import.meta.url))], {
    env: { ...process.env, DEZIRCLIP_NATIVE_CAPTURE_TEXT: text }, windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  await waitForNativeProcess(child, observation);
}
function comparablePath(value) {
  return path.resolve(value.replace(/^\\\\\?\\/, "")).replace(/[\\/]+$/, "").toLowerCase();
}
try {
  for (let attempt = 0; attempt < 180; attempt++) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1500) });
      const targets = await response.json();
      evidence.observations.push({ attempt, status: response.status, targets });
      page = targets.find(target => target.type === "page" && target.webSocketDebuggerUrl && /^(?:https?:\/\/tauri\.localhost(?:[:/]|$)|tauri:\/\/)/.test(target.url) && !/[?&]window=/.test(target.url));
      if (page) break;
    } catch (error) { evidence.observations.push({ attempt, error: String(error), cause: String(error.cause ?? "") }); }
    await delay(250);
  }
  if (!page) throw new Error("Fixture application did not expose its main WebView");
  evidence.page = page;
  const debuggerUrl = new URL(page.webSocketDebuggerUrl);
  assert(debuggerUrl.protocol === "ws:" && ["127.0.0.1", "localhost", "[::1]"].includes(debuggerUrl.hostname) && Number(debuggerUrl.port) === port,
    "Fixture CDP socket must stay on the requested loopback port");
  socket = new WebSocket(page.webSocketDebuggerUrl);
  const rejectPending = () => {
    for (const listener of pending.values()) listener.reject(new Error("Fixture CDP socket closed"));
    pending.clear();
  };
  socket.addEventListener("close", rejectPending);
  socket.addEventListener("error", rejectPending);
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("Fixture CDP socket timed out")), 15000);
    socket.addEventListener("open", () => { clearTimeout(timeout); resolve(); }, { once: true });
    socket.addEventListener("error", error => { clearTimeout(timeout); reject(error); }, { once: true });
  });
  socket.addEventListener("message", event => {
    const response = JSON.parse(event.data);
    const listener = pending.get(response.id);
    if (!listener) return;
    pending.delete(response.id);
    if (response.error) listener.reject(new Error(JSON.stringify(response.error)));
    else listener.resolve(response.result);
  });
  await poll("Actual invoke bridge", () => evaluate("typeof window.__TAURI_INTERNALS__?.invoke === 'function'"));
  const version = await invoke("plugin:app|version");
  assert.equal(version, configuration.expectedVersion, "Installed GUI reported the wrong built-in app version");
  const buildInfo = await invoke("get_build_info");
  assert.equal(buildInfo.component, "desktop");
  assert.equal(buildInfo.component_version, version);
  assert.equal(buildInfo.build_id, configuration.expectedBuildInfo.build_id, "GUI and packaged CLI belong to different build sets");
  assert.equal(buildInfo.source_revision, configuration.expectedBuildInfo.source_revision);
  assert.equal(buildInfo.source_state, configuration.expectedBuildInfo.source_state);
  assert.equal(buildInfo.metadata_version, configuration.expectedBuildInfo.metadata_version);
  assert.equal(buildInfo.target, configuration.expectedBuildInfo.target);
  assert.deepEqual(buildInfo.linked_components, configuration.expectedBuildInfo.linked_components);
  assert.deepEqual(buildInfo.protocol_version, configuration.expectedBuildInfo.protocol_version);
  assert.equal(buildInfo.database_schema_version, configuration.expectedBuildInfo.database_schema_version);
  const dataPath = await invoke("get_data_path");
  assert.equal(comparablePath(dataPath), comparablePath(configuration.profileDirectory), "GUI did not open the shared synthetic profile");
  const oldMarker = (await history()).filter(entry => entry.content === configuration.marker);
  assert.equal(oldMarker.length, 1, "Actual GUI did not preserve exactly one old marker");
  await invoke("focus_clipboard_window");
  await poll("Rendered old history", () => evaluate(`document.body?.innerText.includes(${JSON.stringify(configuration.marker)})`));
  evidence.gui = { version, buildInfo, dataPath, oldMarkerId: oldMarker[0].id, oldHistoryRendered: true };
  if (configuration.acquireOwnership) {
    const result = await evaluate(`(async () => {
      const [changed, token] = await window.__TAURI_INTERNALS__.invoke("begin_registry_win_v_change", { enable: true });
      if (typeof token !== "string" || token.length === 0) throw new Error("Missing fixture change token");
      await window.__TAURI_INTERNALS__.invoke("finish_registry_win_v_change", { token, rollback: false });
      return { changed, optimized: await window.__TAURI_INTERNALS__.invoke("is_registry_win_v_optimized") };
    })()`);
    assert(result?.changed && result.optimized, "Actual GUI did not acquire the fixture settings");
    evidence.ownership = result;
  }
  if (configuration.verifyCapture) {
    await invoke("set_persistence", { enabled: true });
    await invoke("set_deduplication", { enabled: true });
    await invoke("set_capture_rich_text", { enabled: false });
    await invoke("set_privacy_protection", { enabled: false });
    await invoke("set_sequential_mode", { enabled: false });
    await invoke("set_idle_destroy_enabled", { enabled: false });
    await invoke("focus_clipboard_window");
    await evaluate(`(async () => {
      window.__dezirclipNativeAcceptance = { captured: [], legacy: [], listeners: [], overflow: false };
      for (const event of ["clipboard-captured", "clipboard-updated"]) {
        const handler = window.__TAURI_INTERNALS__.transformCallback(value => {
          const state = window.__dezirclipNativeAcceptance;
          if (state.captured.length + state.legacy.length >= 256) { state.overflow = true; return; }
          if (event === "clipboard-captured") state.captured.push(value.payload);
          else state.legacy.push({ id: value.payload?.id, keys: Object.keys(value.payload ?? {}) });
        });
        const eventId = await window.__TAURI_INTERNALS__.invoke("plugin:event|listen", { event, target: { kind: "Any" }, handler });
        window.__dezirclipNativeAcceptance.listeners.push({ event, eventId, handler });
      }
      return true;
    })()`);
    await delay(1000);
    const firstText = `${configuration.marker}-native-a`;
    const secondText = `${configuration.marker}-native-b`;
    await setNativeClipboard(firstText);
    const first = await poll("Native first capture", async () => (await history()).find(entry => entry.content === firstText));
    assert(Number.isSafeInteger(first.id) && first.id > 0);
    await poll("Native first capture event", () => evaluate(`window.__dezirclipNativeAcceptance.captured.some(payload => payload?.id === ${first.id})`));
    await setNativeClipboard(secondText);
    const second = await poll("Native intervening capture", async () => (await history()).find(entry => entry.content === secondText));
    assert.notEqual(first.id, second.id);
    await poll("Native intervening capture event", () => evaluate(`window.__dezirclipNativeAcceptance.captured.some(payload => payload?.id === ${second.id})`));
    await delay(650);
    await setNativeClipboard(firstText);
    const repeated = await poll("Native same-ID recapture", async () => {
      const matches = (await history()).filter(entry => entry.content === firstText);
      assert.equal(matches.length, 1, "Native recapture created duplicate history rows");
      return matches[0]?.id === first.id && matches[0].timestamp > first.timestamp ? matches[0] : null;
    });
    await poll("Native repeated capture event", () => evaluate(`window.__dezirclipNativeAcceptance.captured.filter(payload => payload?.id === ${first.id}).length >= 2`));
    const observed = await evaluate("({ captured: window.__dezirclipNativeAcceptance.captured, legacy: window.__dezirclipNativeAcceptance.legacy })");
    const captureHost = await invoke("get_clipboard_host_status");
    assert.equal(typeof captureHost?.state?.service_instance_id, "string");
    assert(captureHost.state.service_instance_id.length > 0);
    assert.equal(await evaluate("window.__dezirclipNativeAcceptance.overflow"), false, "Fixture event evidence overflowed");
    for (const payload of observed.captured) {
      assert(payload && typeof payload === "object" && !Array.isArray(payload));
      assert.deepEqual(Object.keys(payload).sort(), ["host_instance_id", "id"], "Capture event broadcast a field beyond its owner and ID");
      assert(Number.isSafeInteger(payload.id) && payload.id > 0);
      assert.equal(payload.host_instance_id, captureHost.state.service_instance_id, "Capture event did not retain its actual Host instance");
    }
    assert(!observed.legacy.some(payload => payload.id === first.id || payload.id === second.id), "Native capture also broadcast a legacy full-entry event");
    evidence.capture = { firstText, secondText, firstId: first.id, secondId: second.id, repeatedId: repeated.id, firstTimestamp: first.timestamp, repeatedTimestamp: repeated.timestamp, ...observed };
    const retained = (await history()).filter(entry => entry.content === configuration.marker);
    assert.equal(retained.length, 1);
    assert.equal(retained[0].id, oldMarker[0].id);
  }
  evidence.result = "passed";
  process.stdout.write("Actual installed GUI version, shared history and requested native checks completed\n");
} catch (error) {
  evidence.result = "failed";
  evidence.error = String(error?.stack ?? error);
  throw error;
} finally {
  if (socket?.readyState === WebSocket.OPEN) {
    try {
      evidence.finalHistory = await history();
      if (configuration.verifyCapture) evidence.finalEvents = await evaluate("window.__dezirclipNativeAcceptance");
    } catch (error) { evidence.finalReadError = String(error); }
    try {
      const screenshot = await command("Page.captureScreenshot", { format: "png" });
      const screenshotPath = fixturePath(`${output}.png`);
      await writeFile(screenshotPath, Buffer.from(screenshot.data, "base64"));
      evidence.screenshot = screenshotPath;
    } catch (error) { evidence.screenshotError = String(error); }
    if (configuration.verifyCapture) {
      try {
        await evaluate(`(async () => {
          for (const listener of window.__dezirclipNativeAcceptance?.listeners ?? []) {
            await window.__TAURI_INTERNALS__.invoke("plugin:event|unlisten", { event: listener.event, eventId: listener.eventId });
            window.__TAURI_INTERNALS__.unregisterCallback(listener.handler);
          }
          return true;
        })()`);
      } catch (error) { evidence.listenerCleanupError = String(error); }
    }
  }
  await writeFile(output, JSON.stringify(evidence, null, 2));
  socket?.close();
}
