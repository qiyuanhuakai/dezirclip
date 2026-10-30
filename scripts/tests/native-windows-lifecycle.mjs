// All destructive operations use the original spawned ChildProcess object.
export class DeadlineError extends Error {}

export function trackManagedChild(child, record, afterClose = () => {}, timestamp = () => new Date().toISOString(), onStreamError = () => {}) {
  record.process_exit = { observed: false, exit_code: null, signal: null, observed_at: null };
  record.stdio_close = { observed: false, observed_at: null };
  child.once('exit', (exitCode, signal) => {
    record.process_exit = { observed: true, exit_code: exitCode, signal, observed_at: timestamp() };
  });
  for (const name of ['stdin', 'stdout', 'stderr']) child[name]?.on('error', error => {
    const text = String(error).slice(0, 2048);
    const errors = record.stream_errors ??= [];
    if (errors.length < 8) errors.push({ stream: name, error: text });
    record.error = text; onStreamError(error);
  });
  return new Promise(resolve => child.once('close', (exitCode, signal) => {
    const result = { exit_code: exitCode, signal };
    Object.assign(record, result, { closed_at: timestamp() });
    record.stdio_close = { observed: true, observed_at: record.closed_at };
    resolve(result); afterClose();
  }));
}

export async function within(promise, milliseconds, message, timers = globalThis) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = timers.setTimeout(() => reject(new DeadlineError(message)), milliseconds);
    })]);
  } finally { timers.clearTimeout(timer); }
}

export async function finalizeManagedChild(managed, graceMilliseconds, timers = globalThis) {
  const receipt = managed.record.teardown ??= {
    kill_requested: false, kill_accepted: null, stdio_detached: false,
    termination_confirmed: false, close_confirmed: false, close_deadline_exceeded: false, errors: [],
  };
  const wait = async milliseconds => {
    try { await within(managed.closed, milliseconds, 'Original child close deadline expired', timers); }
    catch (error) { if (!(error instanceof DeadlineError)) receipt.errors.push(String(error)); }
  };
  if (!managed.record.closed_at && graceMilliseconds > 0) await wait(graceMilliseconds);
  if (!managed.record.closed_at) {
    receipt.close_deadline_exceeded = true;
    // An observed exit can precede stdio close. Never signal that PID again.
    if (!managed.record.process_exit?.observed) {
      receipt.kill_requested = true;
      try { receipt.kill_accepted = managed.child.kill(); }
      catch (error) { receipt.kill_accepted = false; receipt.errors.push(String(error)); }
    }
    await wait(2000);
  }
  receipt.termination_confirmed = Boolean(managed.record.process_exit?.observed || managed.record.closed_at);
  receipt.close_confirmed = Boolean(managed.record.closed_at);
  if (!receipt.close_confirmed) {
    try { managed.cancelPending?.(new Error('Original child finalization was not confirmed')); }
    catch (error) { receipt.errors.push(String(error)); }
    for (const stream of ['stdin', 'stdout', 'stderr']) {
      try { managed.child[stream]?.destroy(); } catch (error) { receipt.errors.push(String(error)); }
    }
    try { managed.child.unref(); } catch (error) { receipt.errors.push(String(error)); }
    receipt.stdio_detached = true;
  }
  return { ...receipt, errors: [...receipt.errors] };
}

export async function stopManagedChild(managed, stop, stopMilliseconds, timers = globalThis) {
  let stopError, inputError;
  try { await within(Promise.resolve().then(stop), stopMilliseconds, 'Original native stop request timed out', timers); }
  catch (error) { stopError = String(error); }
  try { managed.child.stdin?.end(); } catch (error) { inputError = String(error); }
  const receipt = await finalizeManagedChild(managed, 20000, timers);
  return { ...receipt, stop_error: stopError ?? null, input_error: inputError ?? null };
}

// Reports and raw logs are synchronously sealed before this failure-only path.
// A failed controller must not remain alive on an unconfirmed socket or pipe.
export function exitFailedController(error, runtime = process, timers = globalThis) {
  runtime.exitCode = 1;
  let timer, pending = 2, exited = false;
  const leave = () => {
    if (exited) return;
    exited = true; timers.clearTimeout(timer); runtime.exit(1);
  };
  timer = timers.setTimeout(leave, 1000);
  const flushed = () => {
    if (--pending || exited) return;
    leave();
  };
  try { runtime.stdout.write('', flushed); } catch { flushed(); }
  try { runtime.stderr.write(String(error.stack ?? error) + '\n', flushed); } catch { flushed(); }
}
