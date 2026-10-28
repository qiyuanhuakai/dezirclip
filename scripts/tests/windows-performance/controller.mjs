import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { closeSync, existsSync, fsyncSync, mkdirSync, openSync, realpathSync, writeSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { performance } from 'node:perf_hooks';
import {
  repository, sha, json, delay, normalized, regular, owned, save, copy, canarySnapshot,
  environment, LoggedProcess, NativeChannel, Bridge, completedCommand, availablePort,
  fixedSurfaceInputs, toolkitPatchId, validateCiContext, validateBuildProof,
  validateDesktopIdentity, verifyNativePanel, verifyNativeTauriPanel,
} from './native-plumbing.mjs';
import { ProcessLedger, percentile, SAMPLE_INTERVAL_MS, SETTLE_MS, SAMPLE_MS } from './process-sampler.mjs';

export const ARMS = ['tauri', 'mygo', 'mygo', 'tauri'];
export const WARMUPS = 5;
export const MEASURED = 30;
export const LATENCY_POLL_MS = 10;
export const ARM_ACTIVE_MS = 1020000;
export const ARM_TOTAL_MS = 1080000;

export function dataset(token) {
  assert.match(token, /^[a-z]{8,24}$/);
  return Array.from({ length: 10 }, (_, index) => ({ query: `row${String.fromCharCode(97 + index)}${token}`,
    content: `Performance comparison row ${String.fromCharCode(97 + index)} row${String.fromCharCode(97 + index)}${token} complete ordinary text` }));
}

export function validateRows(history, rows, originalIds = null) {
  assert.equal(history.length, 10, 'Exactly ten ordinary fixture entries are required');
  assert.deepEqual(history.map(row => row.content).sort(), rows.map(row => row.content).sort());
  assert(history.every(row => row.content_type === 'text' && row.tags.length === 0 && !row.is_pinned));
  const ids = history.map(row => row.id).sort((a, b) => a - b);
  assert.equal(new Set(ids).size, 10);
  if (originalIds) assert.deepEqual(ids, originalIds, 'No fixture entry may be consumed or replaced');
  return ids;
}

export function verifyPanel(observation, renderer, desktop, helper, previous = null) {
  assert(!observation.windows.some(window => window.pid === desktop.pid && window.title === 'DezirClip' && window.visible),
    'The retained main WebView must remain hidden during comparison');
  let panel;
  if (renderer === 'mygo') panel = verifyNativePanel(observation, helper, previous?.pid ?? null);
  else {
    const root = observation.processes.find(item => item.pid === desktop.pid);
    assert(root?.running && root.creation_time_100ns === desktop.creation_time_100ns);
    const actual = verifyNativeTauriPanel(observation, desktop.pid);
    panel = { pid: desktop.pid, creation_time_100ns: desktop.creation_time_100ns,
      hwnd: actual.hwnd, rect: actual.rect };
  }
  assert.equal(panel.rect.width, 480);
  assert.equal(panel.rect.height, 360);
  if (previous) for (const field of ['pid', 'creation_time_100ns', 'hwnd']) assert.equal(panel[field], previous[field]);
  return panel;
}

export function latencyMs(input, observation) {
  const delta = BigInt(observation.observed_monotonic_ns) - BigInt(input.input_monotonic_ns);
  assert(delta >= 0n && delta <= 12000000000n, 'Native endpoint must be in the original input budget');
  return Number(delta) / 1e6;
}

export function remaining(deadline, limit, now = performance.now()) {
  const budget = Math.min(limit, deadline - now);
  assert(budget > 0, 'Original arm deadline expired');
  return Math.max(1, Math.floor(budget));
}

export function validateResourceState(value, panel, mainHwnd, visible) {
  assert.equal(value.panel.hwnd, panel.hwnd); assert.equal(value.panel.pid, panel.pid);
  assert.equal(value.panel.visible, visible); assert.equal(value.panel.foreground, visible);
  assert.equal(value.main.hwnd, mainHwnd); assert.equal(value.main.visible, false);
  if (!visible) assert.equal(value.foreground_target, true);
}

function assertHost(status, original = null) {
  assert.equal(status.mode, 'embedded'); assert.equal(status.capture_owner, true);
  assert.equal(status.privacy.active, false);
  if (original) assert.equal(status.state.service_instance_id, original.state.service_instance_id);
}

async function boundedPoll(label, action, armDeadline, budget = 12000, interval = 120) {
  const deadline = Math.min(armDeadline, performance.now() + budget);
  let last;
  do {
    assert(performance.now() < armDeadline, 'Original arm active deadline expired');
    try { const result = await action(remaining(deadline, 12000)); if (result) return result; } catch (error) { last = error; }
    await delay(Math.max(0, Math.min(interval, deadline - performance.now())));
  } while (performance.now() < deadline);
  throw new Error(label + ' timed out' + (last ? ': ' + last.message : ''));
}

async function runArm(args, expected, renderer, rows, cohortRoot, armIndex) {
  const started = performance.now(), activeDeadline = started + ARM_ACTIVE_MS;
  const root = owned(path.join(cohortRoot, 'dezirclip-surface-' + randomUUID().replaceAll('-', '')), realpathSync(process.env.RUNNER_TEMP));
  assert(!existsSync(root)); mkdirSync(root);
  const report = { schema_version: 1, kind: 'phase6_windows_comparison_arm_v1', source_revision: expected.source_revision,
    renderer, arm_index: armIndex, status: 'running', observations: [], trials: [], resources: {},
    limits: ['Native visible plus actual foreground is an observed polling endpoint, not first interactive frame.',
      'First invocation follows Desktop startup/settings; Tauri may be prebuilt while MyGo may start a new helper. Neither is whole-application cold startup.',
      'Activation proxy percentiles cannot establish the first-interactive-frame improvement threshold.',
      'Query gets a fixed 700 ms setup settle; no search-ready latency is measured.',
      'Main WebView remains constructed and hidden, idle destruction disabled for both renderers.',
      'Owned process cleanup can be forced; natural application shutdown is not measured.'],
    latency_poll_delay_ms: LATENCY_POLL_MS,
    first_interactive_frame: null, search_ready: null, ime: null, dpi: null, multi_monitor: null };
  let native, desktop, bridge, canary, sourceInputs = [];
  const request = (operation, data = {}, limit = 15000) => native.request(operation, data, remaining(activeDeadline, limit));
  const invoke = (command, data = {}, limit = 20000) => bridge.invoke(command, data, remaining(activeDeadline, limit));
  try {
    const destination = path.join(root, 'candidate'); mkdirSync(destination); mkdirSync(path.join(root, 'profile'));
    const desktopPath = path.join(destination, 'dezirclip.exe'), helper = path.join(destination, 'dzc-surface.exe');
    report.input_bytes = [copy(path.resolve(args.desktop), desktopPath), copy(path.resolve(args.helper), helper),
      copy(path.resolve(args['helper-proof']), path.join(root, 'helper.proof.json'))];
    const proof = json(args['helper-proof']);
    assert.equal(normalized(proof.executable), normalized(path.resolve(args.helper)));
    validateBuildProof(proof, proof.build_info, expected, regular(helper).sha256);
    sourceInputs = proof.inputs.map(item => {
      assert(/^(?:frontends\/dzc-surface\/|clients\/go\/|design\/)/.test(item.path) || fixedSurfaceInputs.has(item.path));
      const file = owned(path.resolve(repository, item.path), repository, true);
      assert.equal(regular(file).sha256, item.sha256); return { path: file, ...regular(file) };
    });
    const metadata = new LoggedProcess(helper, ['--build-info'], root, 'surface-build-info', environment(root));
    await completedCommand(metadata, 10000);
    const info = JSON.parse(metadata.stdout.toString('utf8').trim());
    validateBuildProof(proof, info, expected, regular(helper).sha256); report.surface_build_info = info;
    const canaryDirectory = path.join(destination, '.dzc-surface.old-' + randomUUID()); mkdirSync(canaryDirectory);
    const marker = openSync(path.join(canaryDirectory, 'marker.txt'), 'wx');
    try { writeSync(marker, Buffer.from('Phase6 comparison owned startup canary.\n')); fsyncSync(marker); } finally { closeSync(marker); }
    canary = canarySnapshot(canaryDirectory); report.startup_canary_before = canary;
    const port = await availablePort(); report.cdp_port = port;
    native = new NativeChannel(args.python, root, expected.source_revision, port);
    await Promise.race([native.ready, delay(20000).then(() => { throw new Error('Owned native target bootstrap timed out'); })]);
    const seedText = 'nativeupgradefixturemarker' + randomUUID().replaceAll('-', '') + '-native-a';
    const seed = new LoggedProcess(args.python, ['-B', path.join(repository, 'scripts/tests/native-windows-clipboard.py')], root,
      'synthetic-clipboard-seed', { ...environment(root, true), DEZIRCLIP_NATIVE_CAPTURE_TEXT: seedText });
    await completedCommand(seed, 15000);
    const calibration = await request('target-preflight', { expected_text: seedText });
    assert.equal(calibration.actual_target_text, seedText); assert.equal(calibration.paste_attempts, 1);
    assert.equal(calibration.input.sent, 4);
    assert.equal(calibration.focus_before_input.foreground_hwnd, calibration.focus_before_input.target_hwnd);
    assert.equal(calibration.focus_before_input.own_thread_focus_hwnd, calibration.focus_before_input.edit_hwnd);
    assert.equal((await request('target-text')).text, '');
    report.independent_target_calibration = calibration;
    desktop = new LoggedProcess(desktopPath, [], root, 'desktop', { ...environment(root),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
      WEBVIEW2_USER_DATA_FOLDER: path.join(root, 'webview'), DEZIRCLIP_DB_PATH: path.join(root, 'profile/clipboard.db') });
    assert(Number.isSafeInteger(desktop.child.pid));
    report.desktop_original = await request('bind', { pid: desktop.child.pid });
    bridge = new Bridge(); await bridge.connect(port, report.observations);
    report.desktop_build_info_before = await invoke('get_build_info');
    validateDesktopIdentity(report.desktop_build_info_before, expected);
    assert.equal(normalized(await invoke('get_data_path')), normalized(path.join(root, 'profile')));
    report.host_before = await invoke('get_clipboard_host_status'); assertHost(report.host_before);
    await invoke('focus_clipboard_window');
    await invoke('set_idle_destroy_enabled', { enabled: false });
    await invoke('set_persistence', { enabled: true });
    await invoke('set_delete_after_paste', { enabled: false });
    for (const [key, value] of [['app.quick_paste_hotkey', 'Ctrl+Alt+Shift+F10'], ['app.quick_paste_enabled', 'true'],
      ['app.language', 'en'], ['app.quick_paste_renderer', renderer]]) await invoke('save_setting', { key, value });
    const readHistory = (budget = 20000) => invoke('get_clipboard_history', { limit: 150, offset: 0, contentType: null }, budget);
    await boundedPoll('Initial synthetic capture', async budget => (await readHistory(budget)).some(row => row.content === seedText), activeDeadline);
    await invoke('clear_clipboard_history');
    for (const row of rows) await invoke('add_manual_item', { content: row.content, contentType: 'text', tags: [], expectedPrivacyEpoch: null, confirmPersistence: false });
    report.history_before = await readHistory(); const originalIds = validateRows(report.history_before, rows);
    await invoke('hide_window_cmd');
    report.before_first_show = await request('observe');
    report.initial_panel_state = {
      tauri_matching_windows: report.before_first_show.windows.filter(window => window.pid === report.desktop_original.pid
        && window.title === '' && window.class_name === 'Tauri Window' && window.rect?.width === 480 && window.rect?.height === 360),
      mygo_processes: report.before_first_show.processes.filter(item => normalized(item.image) === normalized(helper)),
      meaning: 'Actual state immediately before first invocation after setup, not whole-application cold startup.',
    };
    let first = null, showIndex = 0;
    const inputRole = panel => renderer === 'mygo' ? { role: 'surface' } : { role: 'tauri', panel_hwnd: panel.hwnd };
    const show = async () => {
      const index = showIndex++; let previousSample = null;
      await request('activate-target');
      const input = await request('keys', { role: 'target', key: 'hotkey' });
      const result = await boundedPoll('Actual panel visible and foreground', async budget => {
        const observedAt = performance.now();
        const observation = await request('observe', {}, budget);
        report.observations.push({ stage: 'show', show_index: index, observation,
          observer_roundtrip_ms: performance.now() - observedAt,
          native_sample_interval_ms: previousSample === null ? null
            : Number(BigInt(observation.observed_monotonic_ns) - previousSample) / 1e6 });
        previousSample = BigInt(observation.observed_monotonic_ns);
        const panel = verifyPanel(observation, renderer, report.desktop_original, helper, first);
        return { panel, observation };
      }, activeDeadline, 12000, LATENCY_POLL_MS);
      first ??= result.panel;
      return { ...result, native_visible_focus_ms: latencyMs(input, result.observation), input };
    };
    const hidden = async panel => boundedPoll('Panel hidden and original target foreground', async budget => {
      const observation = await request('observe', {}, budget);
      return !observation.windows.some(window => window.hwnd === panel.hwnd && window.visible)
        && observation.foreground.hwnd === observation.target_hwnd ? observation : null;
    }, activeDeadline);
    for (let trial = 0; trial < WARMUPS + MEASURED; trial++) {
      assert(performance.now() < activeDeadline);
      await request('clear-target');
      const shown = await show(), selected = rows[7];
      await request('keys', { ...inputRole(shown.panel), key: 'select-all' });
      await request('keys', { ...inputRole(shown.panel), text: selected.query });
      await delay(700);
      const enter = await request('keys', { ...inputRole(shown.panel), key: 'enter' });
      const targetObservations = []; let previousTargetSample = null;
      const target = await boundedPoll('Single Enter pasted the complete selected text', async budget => {
        const observedAt = performance.now();
        const value = await request('target-text', {}, budget);
        targetObservations.push({ observed_monotonic_ns: value.observed_monotonic_ns,
          observer_roundtrip_ms: performance.now() - observedAt,
          native_sample_interval_ms: previousTargetSample === null ? null
            : Number(BigInt(value.observed_monotonic_ns) - previousTargetSample) / 1e6 });
        previousTargetSample = BigInt(value.observed_monotonic_ns);
        return value.text === selected.content ? value : null;
      }, activeDeadline, 12000, LATENCY_POLL_MS);
      await hidden(shown.panel);
      report.trials.push({ trial, warmup: trial < WARMUPS, native_visible_focus_ms: shown.native_visible_focus_ms,
        enter_to_exact_target_ms: latencyMs(enter, target), native_paste_attempts: 1,
        selected_content_sha256: sha(Buffer.from(selected.content)), shown, enter, target, target_observations: targetObservations });
      validateRows(await readHistory(), rows, originalIds);
    }
    const sampleResources = async (name, panel, mainHwnd) => {
      assert(remaining(activeDeadline, ARM_ACTIVE_MS) > SETTLE_MS + SAMPLE_MS + 1000, 'Complete resource interval must fit the arm budget');
      await delay(SETTLE_MS);
      const ledger = new ProcessLedger(report.desktop_original);
      const output = path.join(root, 'resources-' + name + '.jsonl'), fd = openSync(output, 'wx');
      const begin = performance.now(); let next = begin, lastTick = begin;
      try {
        do {
          const now = performance.now();
          assert(now < activeDeadline && now - lastTick <= 1000, 'Resource sampling gap or arm deadline exceeded');
          const value = await request('observe-processes', { panel_hwnd: panel.hwnd, panel_pid: panel.pid, main_hwnd: mainHwnd }, 1000);
          validateResourceState(value, panel, mainHwnd, name === 'visible');
          const sample = ledger.observe(value); writeSync(fd, Buffer.from(JSON.stringify({ ...sample, raw: value }) + '\n'));
          if (ledger.elapsedMs >= SAMPLE_MS) break;
          lastTick = now; next += SAMPLE_INTERVAL_MS;
          await delay(Math.max(0, next - performance.now()));
        } while (true);
        fsyncSync(fd);
      } finally { closeSync(fd); }
      const observation = await request('observe');
      if (name === 'visible') verifyPanel(observation, renderer, report.desktop_original, helper, panel);
      else assert(!observation.windows.some(window => window.hwnd === panel.hwnd && window.visible));
      return { ...ledger.summarize(), raw: { path: output, ...regular(output) }, observation_after: observation };
    };
    const visible = await show();
    const mainWindows = visible.observation.windows.filter(window => window.pid === report.desktop_original.pid && window.title === 'DezirClip');
    assert.equal(mainWindows.length, 1, 'The same retained main window is required for both renderers');
    const mainHwnd = mainWindows[0].hwnd;
    report.resources.visible = await sampleResources('visible', visible.panel, mainHwnd);
    await request('keys', { ...inputRole(visible.panel), key: 'escape' }); await hidden(visible.panel);
    report.resources.hidden = await sampleResources('hidden', visible.panel, mainHwnd);
    report.history_after = await readHistory(); validateRows(report.history_after, rows, originalIds);
    report.host_after = await invoke('get_clipboard_host_status'); assertHost(report.host_after, report.host_before);
    report.desktop_build_info_after = await invoke('get_build_info'); validateDesktopIdentity(report.desktop_build_info_after, expected);
    const measured = report.trials.filter(trial => !trial.warmup);
    assert.equal(measured.length, MEASURED);
    report.timing = Object.fromEntries(['native_visible_focus_ms', 'enter_to_exact_target_ms'].map(field => [field,
      { p50: percentile(measured.map(trial => trial[field]), 0.5), p95: percentile(measured.map(trial => trial[field]), 0.95) }]));
    report.status = 'passed';
  } catch (error) { report.status = 'failed'; report.error = String(error.stack ?? error); }
  finally {
    const errors = [];
    if (bridge?.socket?.readyState === WebSocket.OPEN) {
      try { await bridge.invoke('hide_quick_paste', {}, 5000); await bridge.invoke('save_setting', { key: 'app.quick_paste_renderer', value: 'tauri' }, 5000); }
      catch (error) { errors.push(String(error)); }
      bridge.close();
    }
    if (native) {
      try {
        try { await native.request('stop', {}, 5000); } finally { native.child.stdin.end(); }
        report.native_target_exit = await Promise.race([native.closed, delay(20000).then(() => { throw new Error('Owned cleanup timed out'); })]);
        assert.equal(report.native_target_exit.exit_code, 0); assert(!native.overflow && !native.error);
        report.cleanup = json(path.join(root, 'native-cleanup.json'));
        assert(!report.cleanup.native_error && !report.cleanup.bootstrap_error);
        assert.deepEqual(report.cleanup.native.remaining_owned_processes, []);
        assert.equal(report.cleanup.native.target_closed, true); assert.equal(report.cleanup.native.alternate_target_closed, true);
      } catch (error) { errors.push(String(error)); } finally { native.finish(); }
    }
    if (desktop) {
      try { report.desktop_exit = await Promise.race([desktop.closed, delay(5000).then(() => { throw new Error('Original Desktop remains alive'); })]);
        desktop.finish(); assert(!desktop.overflow && !desktop.error); }
      catch (error) { errors.push(String(error)); }
    }
    try {
      for (const input of report.input_bytes ?? []) for (const file of [input.source, input.path]) assert.deepEqual(regular(file), { bytes: input.bytes, sha256: input.sha256 });
      assert(sourceInputs.every(input => regular(input.path).sha256 === input.sha256));
      if (canary) { report.startup_canary_after = canarySnapshot(canary.directory); assert.deepEqual(report.startup_canary_after, canary); }
      report.inputs_unchanged = true;
    } catch (error) { errors.push(String(error)); }
    report.elapsed_ms = performance.now() - started;
    if (report.elapsed_ms > ARM_TOTAL_MS) errors.push('Original 1080 second arm budget exceeded');
    report.cleanup_errors = errors; if (errors.length) report.status = 'failed';
    report.reference = { path: path.join(root, 'report.json') }; save(report.reference.path, report);
  }
  assert.equal(report.status, 'passed', report.error ?? report.cleanup_errors.join('; '));
  return report;
}

export function validateComparisonContext(env, platform = process.platform, arch = process.arch) {
  validateCiContext(env, platform, env.GITHUB_SHA);
  assert.equal(arch, 'x64'); assert.equal(env.GITHUB_EVENT_NAME, 'workflow_dispatch');
  assert.equal(env.DZC_WINDOWS_PERFORMANCE_ENABLED, 'true', 'Explicit manual comparison opt-in is required');
}

export async function runComparison(args, env = process.env) {
  validateComparisonContext(env);
  assert.equal(normalized(path.resolve(args.desktop)), normalized(path.join(repository, 'target/release/dezirclip.exe')),
    'This comparison is limited to the release Desktop built by its owning job');
  const version = json(path.join(repository, 'package.json')).version;
  const expected = { component_version: version, source_revision: env.GITHUB_SHA, source_state: 'verified_checkout',
    build_id: version + '@' + env.GITHUB_SHA, target: 'x86_64-pc-windows-msvc', toolkit_patch_id: toolkitPatchId };
  const functional = json(args['functional-report']);
  assert.equal(functional.kind, 'phase6_windows_native_functional'); assert.equal(functional.status, 'passed');
  assert.equal(functional.source_revision, expected.source_revision);
  assert.equal(functional.run_id, env.GITHUB_RUN_ID); assert.equal(functional.run_attempt, env.GITHUB_RUN_ATTEMPT);
  validateDesktopIdentity(functional.desktop_build_info_before, expected);
  for (const [arg, name] of [['desktop', 'dezirclip.exe'], ['helper', 'dzc-surface.exe']]) {
    const bound = functional.input_bytes.find(input => path.basename(input.path) === name);
    assert(bound && bound.sha256 === regular(args[arg]).sha256, 'Functional gate must test these exact binaries');
  }
  const quiet = json(args['quiet-note']);
  assert.equal(quiet.source_revision, env.GITHUB_SHA); assert.equal(quiet.run_id, env.GITHUB_RUN_ID); assert.equal(quiet.run_attempt, env.GITHUB_RUN_ATTEMPT);
  assert.equal(quiet.builds_finished, true); assert.equal(quiet.other_owned_measurements_running, false);
  assert.equal(quiet.shared_host_quiet_verified, false, 'A shared runner cannot attest physical host quiet');
  const root = owned(args['output-root'], realpathSync(env.RUNNER_TEMP));
  assert.match(path.basename(root), /^dezirclip-performance-[a-f0-9]{32}$/); assert(!existsSync(root)); mkdirSync(root);
  const rows = dataset(randomUUID().replaceAll('-', '').slice(0, 12).replace(/[0-9]/g, character => String.fromCharCode(97 + Number(character))));
  const report = { schema_version: 1, kind: 'phase6_windows_native_comparison_v1', status: 'running', source_revision: expected.source_revision,
    fixture_id: 'phase6-windows-native-comparison-v1', desktop_build_profile: 'release',
    fixture_sources: ['controller.mjs', 'native-plumbing.mjs', 'native-target.py', 'process-sampler.mjs'].map(name => ({ name,
      ...regular(new URL(name, import.meta.url)) })),
    run_id: env.GITHUB_RUN_ID, run_attempt: env.GITHUB_RUN_ATTEMPT, order: ARMS, rows, arms: [],
    functional_gate: { path: args['functional-report'], ...regular(args['functional-report']) },
    quiet_note: { ...quiet, path: args['quiet-note'], ...regular(args['quiet-note']) },
    first_interactive_frame: null, search_ready: null, gpu: null, wakeups: null, ime: null, dpi: null, multi_monitor: null };
  try {
    for (const [index, renderer] of ARMS.entries()) report.arms.push(await runArm(args, expected, renderer, rows, root, index));
    report.status = 'passed';
  } catch (error) { report.status = 'failed'; report.error = String(error.stack ?? error); }
  finally { save(path.join(root, 'comparison.json'), report); }
  assert.equal(report.status, 'passed', report.error);
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const args = {}, names = ['desktop', 'helper', 'helper-proof', 'python', 'functional-report', 'quiet-note', 'output-root'];
  for (let index = 2; index < process.argv.length; index += 2) {
    const key = process.argv[index].slice(2); assert(names.includes(key) && process.argv[index + 1] && !Object.hasOwn(args, key)); args[key] = process.argv[index + 1];
  }
  assert(names.every(key => args[key]));
  runComparison(args).catch(error => { console.error(error.stack ?? error); process.exitCode = 1; });
}
