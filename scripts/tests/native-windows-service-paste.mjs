import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { closeSync, existsSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, realpathSync, writeSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Bridge, completedCommand, validateCiContext, validateDesktopIdentity } from './native-windows-surface.mjs';
import { trackManagedChild, within, finalizeManagedChild, stopManagedChild, exitFailedController } from './native-windows-lifecycle.mjs';
import { desktopExitVerdict } from './native-windows-crash-events.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const normalized = value => path.resolve(value.replace(/^\\\\\?\\/, '')).toLowerCase();
const json = file => JSON.parse(readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function regular(file) {
  const info = lstatSync(file); assert(info.isFile() && !info.isSymbolicLink());
  return { bytes: info.size, sha256: sha(readFileSync(file)) };
}
function owned(value, boundary) {
  const result = path.resolve(value), relative = path.relative(boundary, result);
  assert(relative && relative !== '..' && !relative.startsWith('..' + path.sep) && !path.isAbsolute(relative));
  for (let current = result; ; current = path.dirname(current)) {
    if (existsSync(current)) {
      assert(!lstatSync(current).isSymbolicLink(), 'Private fixture cannot cross a reparse point');
      assert.equal(normalized(realpathSync(current)), normalized(current));
    }
    if (normalized(current) === normalized(boundary)) break;
  }
  return result;
}
function save(file, value) {
  const bytes = Buffer.from(JSON.stringify(value, null, 2) + '\n'), fd = openSync(file, 'wx');
  try { writeSync(fd, bytes); fsyncSync(fd); } finally { closeSync(fd); }
  assert(readFileSync(file).equals(bytes)); return { path: file, bytes: bytes.length, sha256: sha(bytes) };
}
function copy(source, destination) {
  const receipt = regular(source), fd = openSync(destination, 'wx');
  try { writeSync(fd, readFileSync(source)); fsyncSync(fd); } finally { closeSync(fd); }
  assert.deepEqual(regular(destination), receipt); return { source: path.resolve(source), path: destination, ...receipt };
}
function environment(root) {
  const blocked = /^(?:GH_|GITHUB_|ACTIONS_|GIT_|TAURI_SIGNING_|SIGNING_|DZC_|DEZIRCLIP_|WEBVIEW2_|RUST|CARGO_|PYTHON)/i;
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !blocked.test(key)));
  const home = path.join(root, 'environment');
  for (const directory of ['AppData/Roaming', 'AppData/Local', 'temp']) mkdirSync(path.join(home, directory), { recursive: true });
  Object.assign(env, { HOME: home, USERPROFILE: home, APPDATA: path.join(home, 'AppData/Roaming'),
    LOCALAPPDATA: path.join(home, 'AppData/Local'), TEMP: path.join(home, 'temp'), TMP: path.join(home, 'temp'),
    DEZIRCLIP_DB_PATH: path.join(root, 'profile/clipboard.db'), PYTHONNOUSERSITE: '1', PYTHONDONTWRITEBYTECODE: '1' });
  for (const key of ['GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'GITHUB_REPOSITORY', 'GITHUB_SHA', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_EVENT_NAME', 'RUNNER_TEMP']) env[key] = process.env[key];
  return env;
}
class Managed {
  constructor(executable, argv, root, label, env, channel = false) {
    this.root = root; this.label = label; this.stdout = Buffer.alloc(0); this.stderr = Buffer.alloc(0);
    this.pending = new Map(); this.sequence = 0; this.line = ''; this.overflow = false;
    this.files = ['stdout', 'stderr'].map(stream => openSync(path.join(root, label + '.' + stream + '.log'), 'wx'));
    this.child = spawn(executable, argv, { cwd: repository, env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    this.record = { executable, argv, pid: this.child.pid ?? null, started_at: new Date().toISOString() };
    this.ready = new Promise((resolve, reject) => { this.onReady = resolve; this.rejectReady = reject; });
    this.ready.catch(() => {});
    this.closed = trackManagedChild(this.child, this.record, () => {
      this.cancelPending(new Error('Original child closed'));
      if (this.finishRequested) { try { this.finish(); } catch (error) { this.error = String(error); this.record.log_finalization_error = String(error); } }
    }, undefined,
      error => { this.error = String(error); this.cancelPending(error); });
    this.child.once('error', error => { this.error = String(error); this.cancelPending(error); });
    for (const [index, stream] of ['stdout', 'stderr'].entries()) this.child[stream].on('data', chunk => {
      if (this[stream].length + chunk.length > 4 * 1024 * 1024) { this.overflow = true; this.cancelPending(new Error('Raw log bound exceeded')); return; }
      this[stream] = Buffer.concat([this[stream], chunk]);
      try { writeSync(this.files[index], chunk); }
      catch (error) { this.error = String(error); this.record.log_error = String(error); this.cancelPending(error); return; }
      if (channel && stream === 'stdout') {
        this.line += chunk.toString('utf8');
        while (this.line.includes('\n')) {
          const at = this.line.indexOf('\n'), raw = this.line.slice(0, at); this.line = this.line.slice(at + 1);
          let value;
          try { value = JSON.parse(raw); } catch (error) { this.cancelPending(error); continue; }
          if (value.event === 'ready') this.onReady(value);
          const pending = this.pending.get(value.id);
          if (pending) { this.pending.delete(value.id); clearTimeout(pending.timer); value.error ? pending.reject(new Error(value.error)) : pending.resolve(value.value); }
        }
      }
    });
  }
  request(operation, args = {}, milliseconds = 20000) {
    assert(!this.overflow && !this.error && !this.record.process_exit.observed && !this.record.closed_at, 'Original channel unavailable');
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error('Service gate request deadline: ' + operation)); }, milliseconds);
      this.pending.set(id, { resolve, reject, timer });
      this.child.stdin.write(JSON.stringify({ id, operation, ...args }) + '\n');
    });
  }
  cancelPending(error) {
    this.rejectReady(error);
    for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(error); }
    this.pending.clear();
  }
  finish() {
    if (!this.record.closed_at && !this.record.teardown?.stdio_detached) { this.finishRequested = true; return; }
    if (!this.finished) {
      this.finished = true;
      for (const fd of this.files) { fsyncSync(fd); closeSync(fd); }
      save(path.join(this.root, this.label + '.process.json'), { ...this.record, overflow: this.overflow,
        stdout: regular(path.join(this.root, this.label + '.stdout.log')), stderr: regular(path.join(this.root, this.label + '.stderr.log')) });
    }
  }
}
export function assertNoFixtureInput(before, after) {
  const labels = ['correct_original_target', 'stale_original_target_refused', 'generated_held_target_refused'];
  assert(Number.isSafeInteger(before.observation_epoch) && before.observation_epoch >= 1 && before.observation_epoch <= 3);
  assert.equal(before.observation_label, labels[before.observation_epoch - 1]);
  assert.equal(after.observation_epoch, before.observation_epoch); assert.equal(after.observation_label, before.observation_label);
  assert.deepEqual(after.target, before.target); assert.equal(after.target_text, before.target_text);
  assert.deepEqual(after.alternate, before.alternate);
  assert.equal(after.alternate_edit_hwnd, before.alternate_edit_hwnd);
  assert.equal(after.alternate_text, before.alternate_text); assert.deepEqual(after.callback_errors, []);
  assert(before.focus.messages.length < 64 && after.focus.messages.length < 64, 'Fixture message ring overflow');
  assert.deepEqual(after.focus.messages.slice(0, before.focus.messages.length), before.focus.messages, 'Original message prefix changed');
  const extra = after.focus.messages.slice(before.focus.messages.length);
  assert(!extra.some(message => [0x100, 0x101, 0x104, 0x105, 0x302].includes(message.message)), 'Rejected paste inserted fixture input');
  assert(!extra.some(message => ['target', 'target-edit'].includes(message.role)
    && (message.message === 7 || message.message === 6 && (message.wparam & 0xffff) !== 0)), 'Rejected paste reactivated original target A');
  assert.equal(after.focus.foreground_hwnd, before.focus.foreground_hwnd, 'Rejected paste stole foreground');
  return { new_messages: extra, fixture_input_messages: 0 };
}
function validateRole(info, role, expected) {
  for (const field of ['component_version', 'source_revision', 'source_state', 'build_id', 'target', 'database_schema_version', 'protocol_version']) assert.deepEqual(info[field], expected[field]);
  assert.equal(info.component, role); assert.equal(info.metadata_version, 1);
  const names = ['dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host', ...(role === 'service' ? ['dzc-service'] : [])];
  assert.deepEqual(info.linked_components.map(item => [item.component, item.version]).sort(), names.map(name => [name, expected.component_version]).sort());
}
export async function pollRead(label, read, milliseconds = 12000, now = () => performance.now(), sleep = delay, bound = within) {
  const deadline = now() + milliseconds;
  while (now() < deadline) {
    const remaining = deadline - now();
    if (remaining <= 0) break;
    const result = await bound(Promise.resolve().then(() => read(remaining)), remaining, label + ' read deadline');
    if (now() >= deadline) break;
    if (result) return result;
    await sleep(Math.min(30, deadline - now()));
  }
  throw new Error(label + ' timed out');
}
async function port() {
  const server = createServer(); await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const value = server.address().port; await new Promise(resolve => server.close(resolve)); assert(value >= 1024); return value;
}
export async function runServicePaste(args, env = process.env) {
  validateCiContext(env, process.platform, env.GITHUB_SHA);
  const root = owned(args['output-root'], path.resolve(env.RUNNER_TEMP)); assert(!existsSync(root)); mkdirSync(root);
  mkdirSync(path.join(root, 'candidate')); mkdirSync(path.join(root, 'profile'));
  const report = { schema: 1, kind: 'windows_service_bound_paste_functional', source_revision: env.GITHUB_SHA,
    eligible_for_comparison: false, declared_cases: ['correct_original_target', 'stale_original_target_refused', 'generated_held_target_refused'],
    cases: [], observations: [], started_at: new Date().toISOString(), status: 'running', inputs: [], limits: [
      'No performance, IME, GPU, heap root cause or Phase 6 acceptance claim.',
      'Foreground checks and SendInput are not OS-atomic; counts alone are not delivery.',
      'Stale-copy refusal does not separately read clipboard; generated refusal compares only its unique synthetic payload.',
      'Generated refusal may copy the clipboard before the final target check.'] };
  let native, service, desktop, client, bridge;
  const all = [];
  const managed = (...values) => { const process = new Managed(...values); all.push(process); return process; };
  const stage = async (name, action) => {
    const item = { name, status: 'running', started_at: new Date().toISOString() }; report.cases.push(item);
    try {
      item.observation_epoch = await native.request('begin-observation-epoch', { label: name });
      assert.equal(item.observation_epoch.observation_label, name);
      assert.equal(item.observation_epoch.observation_epoch, report.cases.length);
      item.evidence = await action(); item.status = 'passed';
    } catch (error) { item.status = 'failed'; item.error = String(error.stack ?? error); throw error; }
  };
  try {
    const paths = {};
    for (const [key, name] of [['desktop', 'dezirclip.exe'], ['cli', 'dzc.exe'], ['service', 'dzc-service.exe'], ['client', 'service-paste-client.exe']]) {
      paths[key] = path.join(root, 'candidate', name); report.inputs.push(copy(path.resolve(args[key]), paths[key]));
    }
    for (const name of ['native-windows-service-paste.mjs', 'native-windows-service-paste-target.py', 'native-windows-surface.mjs', 'native-windows-surface-target.py', 'native-windows-lifecycle.mjs']) {
      const source = path.join(repository, 'scripts/tests', name); report.inputs.push({ source, ...regular(source) });
    }
    for (const name of ['crates/dzc-service/examples/windows_service_paste_acceptance.rs', '.github/workflows/ci.yml']) {
      const source = path.join(repository, name); report.inputs.push({ source, ...regular(source) });
    }
    const childEnv = environment(root), database = path.join(root, 'profile/clipboard.db');
    const seedText = 'Service paste synthetic ' + randomUUID().replaceAll('-', '') + ' complete ordinary text';
    const seed = managed(paths.client, ['--seed', database], root, 'service-seed', childEnv);
    seed.child.stdin.end(JSON.stringify({ first: seedText, second: 'Service paste synthetic newer independent row' }) + '\n');
    await completedCommand(seed, 15000);
    const seeded = JSON.parse(seed.stdout.toString('utf8')); report.seed = seeded;
    const expected = seeded.build; assert.equal(expected.source_revision, env.GITHUB_SHA); assert.equal(expected.source_state, 'verified_checkout');
    assert.equal(expected.target, 'x86_64-pc-windows-msvc');
    const identities = {};
    for (const [key, argv] of [['service', ['--build-info']], ['cli', ['build-info', '--json']]]) {
      const metadata = managed(paths[key], argv, root, key + '-build-info', childEnv); metadata.child.stdin.end();
      await completedCommand(metadata, 10000); identities[key] = JSON.parse(metadata.stdout.toString('utf8')); validateRole(identities[key], key, expected);
    }
    report.build_info = identities;
    const cdpPort = await port(); report.cdp_port = cdpPort;
    native = managed(args.python, ['-B', fileURLToPath(new URL('./native-windows-service-paste-target.py', import.meta.url)), '--root', root, '--source', env.GITHUB_SHA, '--port', String(cdpPort)], root, 'service-native-target', childEnv, true);
    const nativeReady = await within(native.ready, 20000, 'Private target/bootstrap startup deadline');
    assert.equal(nativeReady.target_pid, native.child.pid);
    const marker = 'nativeupgradefixturemarker' + randomUUID().replaceAll('-', '') + '-native-a';
    const calibration = managed(args.python, ['-B', path.join(repository, 'scripts/tests/native-windows-clipboard.py')], root, 'service-clipboard-calibration', { ...childEnv, DEZIRCLIP_NATIVE_CAPTURE_TEXT: marker });
    calibration.child.stdin.end(); await completedCommand(calibration, 15000);
    report.target_calibration = await native.request('target-preflight', { expected_text: marker });
    assert.equal(report.target_calibration.actual_target_text, marker); assert.equal(report.target_calibration.paste_attempts, 1);
    await native.request('expect-service-original');
    service = managed(paths.service, ['--database', database, '--no-capture'], root, 'service-original', childEnv);
    save(path.join(root, 'service-identity.json'), { pid: service.child.pid, sha256: regular(paths.service).sha256, build: identities.service });
    report.service_original = await native.request('bind-service-original', { pid: service.child.pid });
    client = managed(paths.client, ['--client', database, String(service.child.pid), String(native.child.pid)], root, 'service-client', childEnv, true);
    report.service_connection = await within(client.ready, 25000, 'Authenticated Service readiness deadline');
    assert.equal(report.service_connection.peer_pid, service.child.pid); assert.equal(report.service_connection.status.mode, 'service');
    assert.equal(report.service_connection.status.capture_owner, false);
    const capability = report.service_connection.capabilities.platform.windows_bound_paste_target;
    assert.equal(capability.supported, true); assert.equal(capability.version, 1); validateRole(capability.build, 'service', expected);
    await native.request('expect-desktop-original');
    desktop = managed(paths.desktop, [], root, 'service-desktop-original', { ...childEnv, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + cdpPort, WEBVIEW2_USER_DATA_FOLDER: path.join(root, 'webview') });
    report.desktop_original = await native.request('bind', { pid: desktop.child.pid });
    bridge = new Bridge(); await bridge.connect(cdpPort, report.observations);
    report.build_info.desktop = await bridge.invoke('get_build_info'); validateDesktopIdentity(report.build_info.desktop, expected);
    assert.equal(normalized(await bridge.invoke('get_data_path')), normalized(path.join(root, 'profile')));
    report.desktop_host = await bridge.invoke('get_clipboard_host_status');
    assert.equal(report.desktop_host.mode, 'service'); assert.equal(report.desktop_host.capture_owner, false);
    assert.equal(report.desktop_host.state.service_instance_id, report.service_connection.status.state.service_instance_id);
    await bridge.invoke('set_idle_destroy_enabled', { enabled: false });
    await bridge.invoke('save_setting', { key: 'app.paste_method', value: 'shift_insert' });
    await stage('correct_original_target', async () => {
      await native.request('clear-target'); await native.request('activate-target');
      const before = await native.request('snapshot');
      await bridge.invoke('copy_to_clipboard', { content: seedText, contentType: 'text', paste: true, id: seeded.seeded_ids[0],
        deleteAfterUse: false, pasteWithFormat: false, moveToTop: false, pasteImageAsBase64: false,
        expectedPrivacyEpoch: report.service_connection.status.state.privacy_epoch, derivedFromId: null,
        expectedHostInstanceId: report.desktop_host.state.service_instance_id });
      const after = await pollRead('Original native target full text', async remaining => {
        const value = await native.request('snapshot', {}, Math.max(1, remaining)); return value.target_text === seedText ? value : null;
      });
      assert.deepEqual(after.target, before.target); assert.equal(after.alternate_text, '');
      assert.equal(after.focus.foreground_hwnd, Number(after.target.hwnd));
      return { before, after, history: await client.request('snapshot'), actual_full_target_text: true };
    });
    await stage('stale_original_target_refused', async () => {
      await native.request('clear-target'); await native.request('activate-target');
      const original = await native.request('snapshot'); const historyBefore = await client.request('snapshot');
      const activation = await native.request('activate-alternate'); const before = await native.request('snapshot');
      assert.equal(before.focus.foreground_hwnd, activation.hwnd); assert.equal(before.focus.own_thread_focus_hwnd, before.alternate_edit_hwnd);
      const result = await client.request('reject-copy', { target: original.target, entry_id: seeded.seeded_ids[0] });
      assert.equal(result.rejected, true);
      const after = await native.request('snapshot'), historyAfter = await client.request('snapshot');
      assert.deepEqual(historyAfter, historyBefore, 'Refused transaction consumed/deleted/reordered history');
      return { original, activation, before, after, result, input: assertNoFixtureInput(before, after), history_before: historyBefore, history_after: historyAfter };
    });
    await stage('generated_held_target_refused', async () => {
      await native.request('activate-target'); const original = await native.request('snapshot');
      const historyBefore = await client.request('snapshot');
      const generatedText = 'Service paste synthetic generated ' + randomUUID().replaceAll('-', '');
      const clipboardBefore = await native.request('clipboard-matches-generated', { expected: generatedText });
      assert.equal(clipboardBefore.matches_expected, false);
      const prepared = await client.request('begin-generated', { target: original.target, text: generatedText });
      const activation = await native.request('activate-alternate'), before = await native.request('snapshot');
      assert.equal(before.focus.foreground_hwnd, activation.hwnd); assert.equal(before.focus.own_thread_focus_hwnd, before.alternate_edit_hwnd);
      const result = await client.request('reject-finish'); assert.equal(result.rejected, true);
      assert.equal(result.inside_conservative_ttl, true); assert.equal(result.same_connection_capability_rechecked, true);
      const clipboardAfter = await native.request('clipboard-matches-generated', { expected: generatedText });
      assert.equal(clipboardAfter.matches_expected, true, 'Generated copy must have succeeded before target refusal');
      const after = await native.request('snapshot'), historyAfter = await client.request('snapshot');
      assert.deepEqual(historyAfter, historyBefore, 'No-capture generated refusal changed history');
      return { original, prepared, activation, before, after, result, clipboard_before: clipboardBefore, clipboard_after: clipboardAfter,
        unique_synthetic_payload: generatedText, actual_unique_generated_clipboard_copy: true,
        input: assertNoFixtureInput(before, after), history_before: historyBefore, history_after: historyAfter };
    });
    report.status = 'passed';
  } catch (error) { report.status = 'failed'; report.error = String(error.stack ?? error); }
  finally {
    const errors = []; report.cleanup_started_at = new Date().toISOString();
    if (bridge) { try { report.bridge_cleanup = await bridge.finalize(); if (!report.bridge_cleanup.close_confirmed) errors.push('CDP close unconfirmed'); } catch (error) { errors.push(String(error)); } }
    if (client && !client.record.process_exit.observed) client.child.stdin.end();
    // Native's retained-original cleanup covers both roots before bootstrap restoration.
    if (native) {
      try {
        report.native_lifecycle = await stopManagedChild(native, () => native.request('stop'), 15000);
        assert(report.native_lifecycle.close_confirmed && !report.native_lifecycle.stop_error);
        assert.equal(native.record.exit_code, 0, 'Original native cleanup exited unsuccessfully');
        report.native_cleanup = json(path.join(root, 'service-native-cleanup.json'));
        assert(!report.native_cleanup.native_error && !report.native_cleanup.bootstrap_error);
        assert.deepEqual(report.native_cleanup.native.remaining_owned_processes, []);
        assert.deepEqual(report.native_cleanup.native.unvalidated_pids, []);
        assert(report.native_cleanup.native.target_closed && report.native_cleanup.native.alternate_target_closed);
      } catch (error) { errors.push('Original target cleanup: ' + String(error)); }
    }
    for (const process of all) {
      try {
        if (!process.record.closed_at) { const lifecycle = await finalizeManagedChild(process, 5000); if (!lifecycle.close_confirmed || lifecycle.kill_requested) errors.push(process.label + ': fallback termination/close failure'); }
        process.finish(); assert(!process.error && !process.overflow);
        if (process === client || process === native) assert.equal(process.record.exit_code, 0, process.label + ': unexpected exit');
        if (process === desktop || process === service) {
          const verdict = desktopExitVerdict(process.record, report.cleanup_started_at, report.native_cleanup?.native?.forced_cleanup_pids ?? []);
          report[process === desktop ? 'desktop_exit_verdict' : 'service_exit_verdict'] = verdict;
          assert(verdict.allowed, process.label + ': unexpected original exit ' + verdict.exit_code_hex);
        }
      } catch (error) { errors.push(process.label + ': ' + String(error)); }
    }
    for (const input of report.inputs) {
      try { assert.deepEqual(regular(input.source), { bytes: input.bytes, sha256: input.sha256 }); if (input.path) assert.deepEqual(regular(input.path), { bytes: input.bytes, sha256: input.sha256 }); }
      catch (error) { errors.push('Input integrity: ' + String(error)); }
    }
    report.cleanup_errors = errors; if (errors.length) report.status = 'failed';
    report.finished_at = new Date().toISOString();
    try { console.log(JSON.stringify(save(path.join(root, 'service-paste-report.json'), report))); }
    catch (error) {
      console.error(JSON.stringify({ original_failure: report.error ?? null, cleanup_errors: errors, report_seal_error: String(error) }));
      throw new Error((report.error ?? 'Service evidence sealing failed') + '; report sealing: ' + String(error));
    }
  }
  assert.equal(report.status, 'passed', report.error ?? report.cleanup_errors.join('; ')); return report;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const args = {};
  for (let index = 2; index < process.argv.length; index += 2) {
    const key = process.argv[index].slice(2); assert(['desktop', 'cli', 'service', 'client', 'python', 'output-root'].includes(key) && process.argv[index + 1] && !Object.hasOwn(args, key)); args[key] = process.argv[index + 1];
  }
  assert(['desktop', 'cli', 'service', 'client', 'python', 'output-root'].every(key => args[key]));
  runServicePaste(args).catch(error => exitFailedController(error));
}
