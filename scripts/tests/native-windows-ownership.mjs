// Runs only against the application in a disposable hosted Windows job.
if (process.env.GITHUB_ACTIONS !== "true" || process.env.RUNNER_ENVIRONMENT !== "github-hosted" || process.platform !== "win32") {
  throw new Error("Native ownership acceptance requires a disposable GitHub-hosted Windows runner");
}
const port = Number(process.argv[2]);
if (!Number.isSafeInteger(port) || port < 1024 || port > 65535) throw new Error("Invalid fixture CDP port");
const delay = (milliseconds) => new Promise(resolve => setTimeout(resolve, milliseconds));
let page;
for (let attempt = 0; attempt < 180; attempt++) {
  try {
    const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then(result => result.json());
    page = targets.find(target => target.type === "page" && target.webSocketDebuggerUrl && /tauri\.localhost|^tauri:/.test(target.url));
    if (page) break;
  } catch { /* The fixture WebView may still be starting. */ }
  await delay(250);
}
if (!page) throw new Error("Fixture application did not expose its WebView");
const socket = new WebSocket(page.webSocketDebuggerUrl);
const pending = new Map();
let sequence = 0;
await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});
socket.addEventListener("message", event => {
  const response = JSON.parse(event.data);
  const listener = pending.get(response.id);
  if (!listener) return;
  pending.delete(response.id);
  if (response.error || response.result?.exceptionDetails) {
    listener.reject(new Error(JSON.stringify(response.error ?? response.result.exceptionDetails)));
  } else listener.resolve(response.result?.result?.value);
});
function evaluate(expression) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => { pending.delete(id); reject(new Error("Fixture CDP operation timed out")); }, 30000);
    pending.set(id, {
      resolve: value => { clearTimeout(timeout); resolve(value); },
      reject: error => { clearTimeout(timeout); reject(error); },
    });
    socket.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  });
}
try {
  let ready = false;
  for (let attempt = 0; attempt < 120; attempt++) {
    ready = await evaluate("typeof window.__TAURI_INTERNALS__?.invoke === 'function'");
    if (ready) break;
    await delay(250);
  }
  if (!ready) throw new Error("Fixture invoke bridge was not initialized");
  const result = await evaluate(`(async () => {
    const [changed, token] = await window.__TAURI_INTERNALS__.invoke("begin_registry_win_v_change", { enable: true });
    if (typeof token !== "string" || token.length === 0) throw new Error("Missing fixture change token");
    await window.__TAURI_INTERNALS__.invoke("finish_registry_win_v_change", { token, rollback: false });
    return { changed, optimized: await window.__TAURI_INTERNALS__.invoke("is_registry_win_v_optimized") };
  })()`);
  if (!result?.changed || !result.optimized) throw new Error("Actual application did not acquire the fixture settings");
  process.stdout.write("Actual application ownership commands completed\n");
} finally { socket.close(); }
