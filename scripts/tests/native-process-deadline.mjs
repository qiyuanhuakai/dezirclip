export const nativeWriterTimeoutMs = 15000;

// This helper observes an already-created QA process. Native clipboard access
// remains behind the disposable-runner and synthetic-marker guards.
export function waitForNativeProcess(child, observation, budgetMs = nativeWriterTimeoutMs) {
  if (!Number.isSafeInteger(budgetMs) || budgetMs <= 0 || budgetMs > nativeWriterTimeoutMs) {
    throw new Error("Native writer deadline must stay within fifteen seconds");
  }
  return new Promise((resolve, reject) => {
    observation.pid = child.pid;
    const retain = (field, chunk) => { observation[field] = (observation[field] + chunk.toString("utf8")).slice(0, 65536); };
    child.stdout?.on("data", chunk => retain("stdout", chunk));
    child.stderr?.on("data", chunk => retain("stderr", chunk));
    let settled = false;
    const finish = (code, error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      observation.finishedAt = new Date().toISOString();
      observation.exitCode = code;
      if (error || code !== 0) {
        if (observation.status !== "timeout") observation.status = "failed";
        reject(error || new Error(`Native clipboard writer failed (${code}): ${observation.stdout} ${observation.stderr}`));
      } else { observation.status = "passed"; resolve(); }
    };
    const timeout = setTimeout(() => {
      observation.status = "timeout";
      finish(null, new Error("Native clipboard writer timed out; partial output retained"));
      setImmediate(() => {
        try { observation.killRequested = child.kill(); }
        catch (error) { observation.killError = String(error); }
        child.stdout?.destroy();
        child.stderr?.destroy();
        child.unref();
      });
    }, budgetMs);
    child.once("error", error => finish(null, error));
    child.once("close", code => finish(code));
  });
}
