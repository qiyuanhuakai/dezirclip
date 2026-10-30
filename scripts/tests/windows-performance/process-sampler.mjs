import assert from 'node:assert/strict';

export const SAMPLE_INTERVAL_MS = 250;
export const SETTLE_MS = 30000;
export const SAMPLE_MS = 240000;

export function percentile(values, quantile) {
  assert(values.length > 0 && values.every(Number.isFinite));
  assert(quantile > 0 && quantile <= 1);
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * quantile) - 1];
}

export class ProcessLedger {
  constructor(desktop) {
    this.desktop = desktop;
    this.known = new Map();
    this.samples = [];
  }
  observe(value) {
    const time = BigInt(value.observed_monotonic_ns);
    const previous = this.samples.at(-1);
    assert(!previous || time > BigInt(previous.monotonic_ns), 'Observation clock must advance');
    assert(!previous || time - BigInt(previous.monotonic_ns) <= 1000000000n, 'Resource sampling gap exceeded one second');
    const root = value.processes.find(item => item.pid === this.desktop.pid);
    assert(root?.running && root.creation_time_100ns === this.desktop.creation_time_100ns && root.image === this.desktop.image,
      'Original Desktop identity must remain alive');
    const seen = new Set();
    let workingSet = 0, privateCommit = 0;
    for (const item of value.processes) {
      assert(Number.isSafeInteger(item.pid) && !seen.has(item.pid));
      seen.add(item.pid);
      assert.equal(item.original_handle_retained, true);
      const cpu = item.cpu_kernel_100ns + item.cpu_user_100ns;
      assert(Number.isSafeInteger(cpu) && cpu >= 0);
      let known = this.known.get(item.pid);
      if (!known) {
        known = { image: item.image, creation: item.creation_time_100ns, initial: cpu, latest: cpu };
        this.known.set(item.pid, known);
      }
      assert.equal(known.image, item.image);
      assert.equal(known.creation, item.creation_time_100ns, 'PID reuse must not change an owned identity');
      assert(cpu >= known.latest, 'Retained process CPU must not decrease');
      known.latest = cpu;
      if (item.running) {
        for (const field of ['working_set_bytes', 'private_commit_bytes']) {
          assert(Number.isSafeInteger(item[field]) && item[field] >= 0, 'A running owned process lacks memory counters');
        }
        workingSet += item.working_set_bytes;
        privateCommit += item.private_commit_bytes;
      }
    }
    assert([...this.known.keys()].every(pid => seen.has(pid)), 'An original retained handle disappeared from observations');
    const cpuDelta = [...this.known.values()].reduce((total, item) => total + item.latest - item.initial, 0);
    const sample = { monotonic_ns: time.toString(), working_set_bytes: workingSet,
      private_commit_bytes: privateCommit, observed_cpu_delta_100ns: cpuDelta,
      running_process_count: value.processes.filter(item => item.running).length,
      retained_identity_count: this.known.size };
    this.samples.push(sample);
    return sample;
  }
  get elapsedMs() {
    if (this.samples.length < 2) return 0;
    return Number(BigInt(this.samples.at(-1).monotonic_ns) - BigInt(this.samples[0].monotonic_ns)) / 1e6;
  }
  summarize() {
    assert(this.samples.length > 1);
    const first = this.samples[0], last = this.samples.at(-1);
    const wallMs = this.elapsedMs;
    assert(wallMs >= SAMPLE_MS, 'Resource window must include the complete measurement duration');
    const summary = { sample_count: this.samples.length, measured_wall_ms: wallMs,
      observed_cpu_delta_100ns: last.observed_cpu_delta_100ns,
      observed_cpu_percent_one_core: last.observed_cpu_delta_100ns / (wallMs * 100),
      cpu_scope: 'Observed retained Desktop descendants; each identity starts at its first observation in this window. Short-lived unseen processes and CPU before first observation are omitted.',
      gpu: null, wakeups: null, pss: null, uss: null };
    for (const field of ['working_set_bytes', 'private_commit_bytes']) {
      const values = this.samples.map(sample => sample[field]);
      summary[field] = { p50: percentile(values, 0.5), p95: percentile(values, 0.95), max: Math.max(...values) };
    }
    return summary;
  }
}
