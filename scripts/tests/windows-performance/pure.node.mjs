import assert from 'node:assert/strict';
import test from 'node:test';
import { ARMS, WARMUPS, MEASURED, dataset, validateRows, verifyPanel, latencyMs,
  remaining, validateResourceState, validateComparisonContext, runComparison } from './controller.mjs';
import { ProcessLedger, percentile, SAMPLE_MS } from './process-sampler.mjs';

const desktop = { pid: 10, image: 'C:/fixture/dezirclip.exe', creation_time_100ns: '100' };
const helper = 'C:/fixture/dzc-surface.exe';
const processRow = (pid = 10, patch = {}) => ({ ...desktop, pid, running: true,
  cpu_kernel_100ns: 100, cpu_user_100ns: 100, original_handle_retained: true,
  working_set_bytes: 1000, private_commit_bytes: 2000, ...patch });
const sample = (ms, processes = [processRow()]) => ({ observed_monotonic_ns: String(BigInt(ms) * 1000000n), processes });
const panelObservation = renderer => ({ processes: [processRow(), processRow(20, { image: helper })],
  windows: [{ hwnd: 1, pid: 10, title: 'DezirClip', visible: false },
    { hwnd: 2, pid: renderer === 'tauri' ? 10 : 20, title: '', class_name: 'Tauri Window', visible: true,
      rect: { width: 480, height: 360 } }], foreground: { hwnd: 2, pid: renderer === 'tauri' ? 10 : 20 } });

test('ABBA uses identical ten complete ordinary rows and preserves IDs within each arm', () => {
  assert.deepEqual(ARMS, ['tauri', 'mygo', 'mygo', 'tauri']);
  assert.equal(WARMUPS, 5); assert.equal(MEASURED, 30);
  const rows = dataset('abcdefgh');
  const history = rows.map((row, index) => ({ id: index + 1, content: row.content, content_type: 'text', tags: [], is_pinned: false }));
  const ids = validateRows(history, rows);
  validateRows([...history].reverse(), rows, ids);
  for (const patch of [{ content: 'truncated' }, { tags: ['sensitive'] }, { is_pinned: true }, { id: 100 }]) {
    assert.throws(() => validateRows([{ ...history[0], ...patch }, ...history.slice(1)], rows, ids));
  }
  assert.throws(() => validateRows(history.slice(1), rows));
});

test('panel timing requires native foreground, matching geometry, warm identity and a hidden main', () => {
  for (const renderer of ['tauri', 'mygo']) {
    const observed = panelObservation(renderer);
    const first = verifyPanel(observed, renderer, desktop, helper);
    verifyPanel(observed, renderer, desktop, helper, first);
    const changed = mutation => { const copy = structuredClone(observed); mutation(copy); return copy; };
    assert.throws(() => verifyPanel(changed(value => { value.foreground = {}; }), renderer, desktop, helper));
    assert.throws(() => verifyPanel(changed(value => { value.windows[0].visible = true; }), renderer, desktop, helper));
    assert.throws(() => verifyPanel(changed(value => { value.windows[1].rect.width = 481; }), renderer, desktop, helper));
    assert.throws(() => verifyPanel(changed(value => { value.windows.push({ ...value.windows[1], hwnd: 3 }); }), renderer, desktop, helper));
    assert.throws(() => verifyPanel(observed, renderer, desktop, helper, { ...first, hwnd: 3 }));
  }
  const wrongClass = panelObservation('tauri'); wrongClass.windows[1].class_name = 'Other';
  assert.throws(() => verifyPanel(wrongClass, 'tauri', desktop, helper));
});

test('latency uses the same native clock and deadlines never silently expand', () => {
  assert.equal(latencyMs({ input_monotonic_ns: '1000000000' }, { observed_monotonic_ns: '1125000000' }), 125);
  assert.throws(() => latencyMs({ input_monotonic_ns: '2' }, { observed_monotonic_ns: '1' }));
  assert.throws(() => latencyMs({ input_monotonic_ns: '0' }, { observed_monotonic_ns: '12000000001' }));
  assert.equal(remaining(100, 50, 80), 20);
  assert.throws(() => remaining(100, 50, 100));
});

test('each resource observation requires the intended panel and main visibility', () => {
  const panel = { pid: 20, hwnd: 2 };
  const visible = { panel: { ...panel, visible: true, foreground: true }, main: { hwnd: 1, visible: false }, foreground_target: false };
  validateResourceState(visible, panel, 1, true);
  const hidden = { ...visible, panel: { ...panel, visible: false, foreground: false }, foreground_target: true };
  validateResourceState(hidden, panel, 1, false);
  assert.throws(() => validateResourceState({ ...visible, main: { hwnd: 1, visible: true } }, panel, 1, true));
  assert.throws(() => validateResourceState({ ...hidden, foreground_target: false }, panel, 1, false));
  assert.throws(() => validateResourceState(visible, { ...panel, hwnd: 3 }, 1, true));
});

test('resource ledger retains exited CPU, excludes exited memory and starts newborn counters at observation', () => {
  const ledger = new ProcessLedger(desktop);
  ledger.observe(sample(0, [processRow(), processRow(20, { image: helper })]));
  const value = ledger.observe(sample(250, [processRow(10, { cpu_kernel_100ns: 200 }),
    processRow(20, { image: helper, cpu_user_100ns: 300, running: false, working_set_bytes: null, private_commit_bytes: null }),
    processRow(30, { image: 'C:/fixture/webview.exe', cpu_kernel_100ns: 900000 })]));
  assert.equal(value.observed_cpu_delta_100ns, 300);
  assert.equal(value.working_set_bytes, 2000);
  assert.equal(value.private_commit_bytes, 4000);
  assert.equal(value.retained_identity_count, 3);
});

test('resource ledger rejects lost handles, reused identities, missing memory and regressing counters', () => {
  for (const changed of [[], [processRow(10, { running: false })], [processRow(10, { creation_time_100ns: '101' })],
    [processRow(10, { working_set_bytes: null })], [processRow(10, { cpu_kernel_100ns: 0 })]]) {
    const ledger = new ProcessLedger(desktop); ledger.observe(sample(0));
    assert.throws(() => ledger.observe(sample(250, changed)));
  }
  const ledger = new ProcessLedger(desktop); ledger.observe(sample(0, [processRow(), processRow(20)]));
  assert.throws(() => ledger.observe(sample(250)));
});

test('resource summary requires a complete native-clock duration and reports one-core CPU units', () => {
  const ledger = new ProcessLedger(desktop);
  ledger.observe(sample(0));
  assert.throws(() => ledger.summarize());
  for (let ms = 500; ms <= SAMPLE_MS; ms += 500) ledger.observe(sample(ms, [processRow(10, { cpu_kernel_100ns: 100 + ms * 1000 })]));
  const summary = ledger.summarize();
  assert.equal(summary.measured_wall_ms, SAMPLE_MS); assert.equal(summary.observed_cpu_percent_one_core, 10);
  assert.equal(summary.gpu, null); assert.equal(summary.pss, null);
  assert.equal(percentile(Array.from({ length: 30 }, (_, i) => i + 1), .95), 29);
  const gap = new ProcessLedger(desktop); gap.observe(sample(0));
  assert.throws(() => gap.observe(sample(1001)));
});

test('comparison refuses a non-CI host before reading inputs or creating output', async () => {
  await assert.rejects(() => runComparison({}, { GITHUB_ACTIONS: 'false', GITHUB_SHA: 'a'.repeat(40) }));
  const valid = { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted', GITHUB_REPOSITORY: 'qiyuanhuakai/dezirclip',
    GITHUB_SHA: 'a'.repeat(40), GITHUB_RUN_ID: '12', GITHUB_RUN_ATTEMPT: '1', GITHUB_EVENT_NAME: 'workflow_dispatch',
    RUNNER_TEMP: 'C:/runner/temp', DZC_WINDOWS_PERFORMANCE_ENABLED: 'true' };
  assert.doesNotThrow(() => validateComparisonContext(valid, 'win32', 'x64'));
  for (const patch of [{ GITHUB_EVENT_NAME: 'pull_request' }, { DZC_WINDOWS_PERFORMANCE_ENABLED: 'false' },
    { RUNNER_ENVIRONMENT: 'self-hosted' }]) assert.throws(() => validateComparisonContext({ ...valid, ...patch }, 'win32', 'x64'));
  assert.throws(() => validateComparisonContext(valid, 'win32', 'arm64'));
});
