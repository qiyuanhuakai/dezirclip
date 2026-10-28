import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { closeSync, existsSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, readdirSync, realpathSync, renameSync, writeSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { performance } from 'node:perf_hooks';
import { verifySurfaceIdentity } from '../build-surface.mjs';
import { mainWebviewTargets } from '../release-windows-upgrade.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const json = file => JSON.parse(readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const normalized = value => path.resolve(value.replace(/^\\\\\?\\/, '')).toLowerCase();
const nativeSource = fileURLToPath(new URL('./native-windows-surface-target.py', import.meta.url));
const maximumLog = 16 * 1024 * 1024;
const toolkitPatchId = 'mygo-v0.6.4-no-startup-cleanup-v1';
const upstreamAppHash = 'af53a7f7c0dbe36d42edc6de34f9f7735569ae53170715264af6618b99e281b9';
const patchedAppHash = 'e79050f98a9aae53d3413e0680eb91e66d3d18cc677163acca97ed101ac1fa68';
const fixedSurfaceInputs = new Set(['package.json', 'Cargo.toml', 'Cargo.lock', 'src-tauri/tauri.conf.json',
  'scripts/set-release-version.mjs', 'scripts/updater-release.mjs', 'scripts/build-surface.mjs',
  'scripts/mygo-surface-patch.mjs', 'scripts/generate-surface-tokens.mjs']);

export function validateCiContext(env, platform, source) {
  assert.equal(platform, 'win32', 'Surface native QA only runs on Windows');
  assert.equal(env.GITHUB_ACTIONS, 'true');
  assert.equal(env.RUNNER_ENVIRONMENT, 'github-hosted');
  assert.equal(env.GITHUB_REPOSITORY, 'qiyuanhuakai/dezirclip');
  assert.match(source, /^[a-f0-9]{40}$/);
  assert.equal(env.GITHUB_SHA, source, 'Native QA must use the actual workflow checkout');
  for (const field of ['GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT']) assert.match(env[field] ?? '', /^[1-9][0-9]*$/);
  assert(['push', 'pull_request', 'workflow_dispatch'].includes(env.GITHUB_EVENT_NAME));
  assert.equal(typeof env.RUNNER_TEMP, 'string');
}

export function validateBuildProof(proof, info, expected, binaryHash) {
  assert.equal(proof.schema_version, 1);
  assert.equal(proof.kind, 'surface-build');
  assert.equal(proof.identity_verification, 'executed_native_build_info');
  assert.equal(proof.sha256, binaryHash);
  assert.deepEqual(proof.expected, expected);
  verifySurfaceIdentity(info, expected);
  assert.equal(info.mygo_source_revision, '6255ea40139d06e0762a7c4a32626148ba410478');
  assert.equal(info.toolkit_patch_id, toolkitPatchId);
  assert.equal(proof.expected.toolkit_patch_id, toolkitPatchId);
  assert.equal(proof.toolkit_patch?.id, toolkitPatchId);
  assert.equal(proof.toolkit_patch.strategy, 'isolated_module_copy');
  assert.equal(proof.toolkit_patch.upstream_app_sha256, upstreamAppHash);
  assert.equal(proof.toolkit_patch.patched_app_sha256, patchedAppHash);
  assert.deepEqual(proof.toolkit_patch.changed_files, ['app.go']);
  assert.deepEqual(proof.build_info, info);
  assert(Array.isArray(proof.inputs) && proof.inputs.length > 0, 'Actual Surface source inputs are required');
}

export function validateDesktopIdentity(info, expected) {
  for (const field of ['component_version', 'source_revision', 'source_state', 'build_id', 'target']) assert.equal(info[field], expected[field]);
  assert.equal(info.component, 'desktop'); assert.equal(info.metadata_version, 1); assert.equal(info.database_schema_version, 24);
  assert(['explicit_checkout', 'local_git'].includes(info.revision_origin));
  assert.deepEqual(info.protocol_version, { major: 1, minor: 0 });
  assert.deepEqual(info.linked_components.map(item => [item.component, item.version]).sort(),
    ['dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host'].map(name => [name, expected.component_version]).sort());
}

export function verifyPresented(status, observation, helper, expectedPid = null) {
  assert.equal(status.renderer, 'mygo');
  assert.equal(status.visible, true);
  assert(Number.isSafeInteger(status.managed_pid) && status.managed_pid > 0);
  assert.equal(typeof status.session_id, 'string');
  assert(status.session_id.length > 0);
  if (expectedPid !== null) assert.equal(status.managed_pid, expectedPid, 'Warm show must reuse the actual process');
  return { ...verifyNativePanel(observation, helper, status.managed_pid), session_id: status.session_id };
}

export function verifyNativePanel(observation, helper, expectedPid = null) {
  const process = observation.processes.filter(item => item.running && normalized(item.image) === normalized(helper));
  assert.equal(process.length, 1);
  if (expectedPid !== null) assert.equal(process[0].pid, expectedPid, 'Warm show must reuse the actual process');
  const visible = observation.windows.filter(item => item.pid === process[0].pid && item.visible);
  assert.equal(visible.length, 1, 'Exactly one actual native Surface window must be visible');
  assert.equal(observation.foreground?.pid, process[0].pid, 'The actual Surface must own foreground');
  assert(visible[0].rect?.width > 0 && visible[0].rect?.height > 0);
  return { pid: process[0].pid, creation_time_100ns: process[0].creation_time_100ns,
    hwnd: visible[0].hwnd, rect: visible[0].rect };
}

export function verifyNativeTauriPanel(observation, desktopPid) {
  assert(observation.processes.some(item => item.pid === desktopPid && item.running), 'Original Desktop must remain alive');
  const visible = observation.windows.filter(item => item.pid === desktopPid && item.visible && item.class_name === 'Tauri Window');
  assert.equal(visible.length, 1, 'Exactly one actual Tauri panel must be visible');
  const panel = visible[0];
  assert.equal(panel.title, '');
  assert.equal(panel.class_name, 'Tauri Window');
  assert(panel.rect?.width > 0 && panel.rect?.height > 0);
  assert.equal(observation.foreground?.pid, desktopPid, 'The actual Tauri panel must own foreground');
  assert.equal(observation.foreground?.hwnd, panel.hwnd, 'Another Desktop window cannot substitute for the panel');
  return panel;
}

export function verifyMainDestroyed(observation, desktopPid, originalHwnd) {
  assert(observation.processes.some(item => item.pid === desktopPid && item.running), 'Original Desktop must remain alive');
  assert(!observation.windows.some(item => item.pid === desktopPid && (item.hwnd === originalHwnd || item.title === 'DezirClip')),
    'The original main HWND must be destroyed and no replacement main may exist');
}

export function mainBridgeReady(snapshot) {
  return snapshot?.invoke_type === 'function' && ['interactive', 'complete'].includes(snapshot.ready_state)
    && typeof snapshot.href === 'string' && mainWebviewTargets([
      { type: 'page', url: snapshot.href, webSocketDebuggerUrl: 'observed-main-context' },
    ]).length === 1;
}

export function pasteDiagnosticDue(attempt, recorded) {
  assert(Number.isSafeInteger(attempt) && attempt > 0 && Number.isSafeInteger(recorded) && recorded >= 0);
  return recorded < 16 && (attempt === 1 || attempt % 8 === 0);
}

export function pasteDiagnosticObservation(attempt, statusResult, nativeResult, expectedPid) {
  const result = { stage: 'post-enter-paste-pending', attempt, expected_pid: expectedPid };
  if (statusResult.status === 'fulfilled') result.status = statusResult.value;
  else result.status_error = String(statusResult.reason?.message ?? statusResult.reason).slice(0, 2048);
  if (nativeResult.status === 'fulfilled') {
    const observed = nativeResult.value;
    const pids = new Set([expectedPid, result.status?.managed_pid].filter(pid => Number.isSafeInteger(pid) && pid > 0));
    result.foreground = observed.foreground;
    result.target_hwnd = observed.target_hwnd;
    result.edit_hwnd = observed.edit_hwnd;
    result.helper_windows = observed.windows.filter(window => pids.has(window.pid)).slice(0, 32);
    result.helper_processes = observed.processes.filter(process => pids.has(process.pid)).slice(0, 8);
  } else result.native_error = String(nativeResult.reason?.message ?? nativeResult.reason).slice(0, 2048);
  return result;
}

function regular(file) {
  const stat = lstatSync(file);
  assert(stat.isFile() && !stat.isSymbolicLink(), 'A regular source file is required');
  return { bytes: stat.size, sha256: sha(readFileSync(file)) };
}

function owned(value, boundary, existing = false) {
  const result = path.resolve(value);
  const relative = path.relative(boundary, result);
  assert(relative && relative !== '..' && !relative.startsWith('..' + path.sep) && !path.isAbsolute(relative), 'Path must stay inside its owned fixture');
  for (let current = result; ; current = path.dirname(current)) {
    if (existsSync(current)) {
      assert(!lstatSync(current).isSymbolicLink(), 'A fixture cannot cross a link or junction');
      assert.equal(normalized(realpathSync(current)), normalized(current), 'A fixture path must resolve without redirection');
    }
    if (normalized(current) === normalized(boundary)) break;
  }
  if (existing) assert(existsSync(result));
  return result;
}

function save(file, value) {
  const data = Buffer.from(JSON.stringify(value, null, 2) + '\n');
  const fd = openSync(file, 'wx');
  try { writeSync(fd, data); fsyncSync(fd); } finally { closeSync(fd); }
  assert(readFileSync(file).equals(data));
  return { path: file, bytes: data.length, sha256: sha(data) };
}

function copy(source, destination) {
  const expected = regular(source);
  const fd = openSync(destination, 'wx');
  try { writeSync(fd, readFileSync(source)); fsyncSync(fd); } finally { closeSync(fd); }
  assert.deepEqual(regular(destination), expected);
  assert.deepEqual(regular(source), expected);
  return { source, path: destination, ...expected };
}

function canarySnapshot(directory) {
  const stat = lstatSync(directory, { bigint: true });
  assert(stat.isDirectory() && !stat.isSymbolicLink(), 'Owned startup canary directory changed');
  assert.deepEqual(readdirSync(directory), ['marker.txt'], 'Owned startup canary entries changed');
  const file = path.join(directory, 'marker.txt'), marker = lstatSync(file);
  assert(marker.isFile() && !marker.isSymbolicLink() && marker.size <= 256, 'Owned startup canary marker changed');
  const data = readFileSync(file);
  return { directory, device: stat.dev.toString(), inode: stat.ino.toString(), marker_bytes: data.length,
    marker_sha256: sha(data), marker_utf8: data.toString('utf8') };
}

function environment(root, metadata = false) {
  const blocked = /^(?:GH_|GITHUB_|ACTIONS_|GIT_|TAURI_SIGNING_|SIGNING_|DZC_|DEZIRCLIP_|WEBVIEW2_|RUST|CARGO_|PYTHON)/i;
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !blocked.test(key)));
  const profile = path.join(root, 'environment');
  for (const directory of ['AppData/Roaming', 'AppData/Local', 'temp']) mkdirSync(path.join(profile, directory), { recursive: true });
  Object.assign(env, { HOME: profile, USERPROFILE: profile, APPDATA: path.join(profile, 'AppData/Roaming'),
    LOCALAPPDATA: path.join(profile, 'AppData/Local'), TEMP: path.join(profile, 'temp'), TMP: path.join(profile, 'temp'),
    PYTHONNOUSERSITE: '1', PYTHONDONTWRITEBYTECODE: '1' });
  if (metadata) for (const key of ['GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'GITHUB_REPOSITORY', 'GITHUB_SHA',
    'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_EVENT_NAME', 'RUNNER_TEMP']) env[key] = process.env[key];
  return env;
}

class LoggedProcess {
  constructor(executable, argv, root, label, env, pipeInput = false) {
    this.root = root; this.label = label;
    this.stdout = Buffer.alloc(0); this.stderr = Buffer.alloc(0); this.overflow = false;
    this.files = ['stdout', 'stderr'].map(stream => openSync(path.join(root, label + '.' + stream + '.log'), 'wx'));
    this.child = spawn(executable, argv, { env, cwd: repository, windowsHide: true, stdio: [pipeInput ? 'pipe' : 'ignore', 'pipe', 'pipe'] });
    this.record = { executable, argv, pid: this.child.pid ?? null, started_at: new Date().toISOString() };
    this.closed = new Promise(resolve => this.child.once('close', (exitCode, signal) => {
      const result = { exit_code: exitCode, signal };
      Object.assign(this.record, result, { closed_at: new Date().toISOString() });
      if (this.finishRequested) this.finish();
      resolve(result);
    }));
    this.error = null;
    this.child.once('error', error => { this.error = String(error); this.record.error = this.error; });
    for (const [index, stream] of ['stdout', 'stderr'].entries()) this.child[stream].on('data', chunk => {
      if (this[stream].length + chunk.length > maximumLog) { this.overflow = true; return; }
      this[stream] = Buffer.concat([this[stream], chunk]); writeSync(this.files[index], chunk);
    });
  }
  finish() {
    if (!this.record.closed_at) { this.finishRequested = true; return; }
    if (!this.logsClosed) {
      this.logsClosed = true;
      for (const fd of this.files) { fsyncSync(fd); closeSync(fd); }
      save(path.join(this.root, this.label + '.process.json'), { ...this.record, log_overflow: this.overflow,
        stdout: regular(path.join(this.root, this.label + '.stdout.log')),
        stderr: regular(path.join(this.root, this.label + '.stderr.log')) });
    }
  }
}

async function completedCommand(process, milliseconds) {
  let timer;
  try {
    const result = await Promise.race([process.closed, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('Original metadata command exceeded its deadline')), milliseconds);
    })]);
    assert(!process.error && !process.overflow);
    assert.equal(result.exit_code, 0);
    return result;
  } catch (error) {
    process.child.kill();
    await process.closed;
    throw error;
  } finally {
    clearTimeout(timer);
    process.finish();
  }
}

class NativeChannel extends LoggedProcess {
  constructor(python, root, source, port) {
    super(python, ['-B', nativeSource, '--root', root, '--source', source, '--port', String(port)], root, 'native-target', environment(root, true), true);
    this.sequence = 0; this.pending = new Map(); this.line = '';
    this.ready = new Promise((resolve, reject) => { this.onReady = resolve; this.readyError = reject; });
    this.child.stdout.on('data', chunk => {
      this.line += chunk.toString('utf8');
      while (this.line.includes('\n')) {
        const index = this.line.indexOf('\n'), raw = this.line.slice(0, index); this.line = this.line.slice(index + 1);
        let value;
        try { value = JSON.parse(raw); } catch (error) { this.readyError(error); continue; }
        if (value.event === 'ready') this.onReady(value);
        const pending = this.pending.get(value.id);
        if (pending) { this.pending.delete(value.id); clearTimeout(pending.timeout); value.error ? pending.reject(new Error(value.error)) : pending.resolve(value.value); }
      }
    });
    this.closed.then(result => {
      this.readyError(new Error('Native target exited: ' + JSON.stringify(result)));
      for (const pending of this.pending.values()) { clearTimeout(pending.timeout); pending.reject(new Error('Native target exited')); }
      this.pending.clear();
    });
  }
  request(operation, args = {}, milliseconds = 15000) {
    assert(!this.overflow || operation === 'stop', 'Native raw observation log exceeded bound');
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { this.pending.delete(id); reject(new Error('Native operation timed out: ' + operation)); }, milliseconds);
      this.pending.set(id, { resolve, reject, timeout });
      this.child.stdin.write(JSON.stringify({ id, operation, ...args }) + '\n');
    });
  }
}

class Bridge {
  async connect(port, observations) {
    const started = performance.now(), deadline = started + 45000;
    const target = await poll('Actual main WebView', async () => {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1500) });
      assert.equal(response.status, 200);
      const value = await response.json(), targets = mainWebviewTargets(value);
      observations.push({ stage: 'CDP-discovery', targets: value });
      assert(targets.length <= 1, 'More than one main WebView');
      return targets[0];
    }, Math.max(1, deadline - performance.now()));
    const url = new URL(target.webSocketDebuggerUrl);
    assert.equal(url.protocol, 'ws:'); assert(['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname));
    assert.equal(Number(url.port), port);
    this.socket = new WebSocket(url);
    this.pending = new Map(); this.sequence = 0;
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Actual CDP connection timed out')), Math.max(1, Math.min(10000, deadline - performance.now())));
      this.socket.addEventListener('open', () => { clearTimeout(timeout); resolve(); }, { once: true });
      this.socket.addEventListener('error', () => { clearTimeout(timeout); reject(new Error('Actual CDP connection failed')); }, { once: true });
    });
    this.socket.addEventListener('message', event => {
      const response = JSON.parse(event.data), pending = this.pending.get(response.id);
      if (pending) { this.pending.delete(response.id); clearTimeout(pending.timeout); response.error ? pending.reject(new Error(JSON.stringify(response.error))) : pending.resolve(response.result); }
    });
    this.socket.addEventListener('close', () => { for (const pending of this.pending.values()) { clearTimeout(pending.timeout); pending.reject(new Error('Actual CDP connection closed')); } this.pending.clear(); });
    let readinessCount = 0;
    await poll('Actual main Tauri IPC readiness', async () => {
      assert(++readinessCount <= 384, 'Actual main readiness observation bound exceeded');
      const observation = { stage: 'CDP-main-readiness', elapsed_ms_since_connect: performance.now() - started };
      try {
        const remaining = deadline - performance.now();
        assert(remaining > 0, 'Actual main readiness deadline exceeded');
        const result = await this.command('Runtime.evaluate', {
          expression: '({href:location.href,ready_state:document.readyState,invoke_type:typeof window.__TAURI_INTERNALS__?.invoke})',
          returnByValue: true,
        }, Math.max(1, Math.min(2000, remaining)));
        if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
        const snapshot = result.result?.value;
        observation.snapshot = snapshot;
        observations.push(observation);
        return mainBridgeReady(snapshot) ? snapshot : null;
      } catch (error) {
        observation.error = String(error.message ?? error).slice(0, 2048);
        observations.push(observation);
        return null;
      }
    }, Math.max(1, deadline - performance.now()));
  }
  command(method, params, milliseconds = 20000) {
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { this.pending.delete(id); reject(new Error('Actual CDP operation timed out')); }, milliseconds);
      this.pending.set(id, { resolve, reject, timeout }); this.socket.send(JSON.stringify({ id, method, params }));
    });
  }
  async invoke(name, args = {}, milliseconds = 20000) {
    const result = await this.command('Runtime.evaluate', { expression: `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(name)},${JSON.stringify(args)})`, awaitPromise: true, returnByValue: true }, milliseconds);
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result?.value;
  }
  close() { this.socket?.close(); }
}

async function poll(label, action, milliseconds = 12000, absoluteDeadline = null) {
  const deadline = absoluteDeadline ?? performance.now() + milliseconds;
  let last;
  do { try { const value = await action(); if (value) return value; } catch (error) { last = error; } await delay(120); } while (performance.now() < deadline);
  throw new Error(label + ' timed out' + (last ? ': ' + last.message : ''));
}

async function availablePort() {
  const server = createServer();
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  assert(port >= 1024); return port;
}

export async function runSurfaceAcceptance(args, env = process.env) {
  const expected = { component_version: json(path.join(repository, 'package.json')).version, source_revision: env.GITHUB_SHA,
    source_state: 'verified_checkout', build_id: json(path.join(repository, 'package.json')).version + '@' + env.GITHUB_SHA,
    target: process.arch === 'arm64' ? 'aarch64-pc-windows-msvc' : 'x86_64-pc-windows-msvc', toolkit_patch_id: toolkitPatchId };
  validateCiContext(env, process.platform, expected.source_revision);
  const root = owned(args['output-root'], realpathSync(env.RUNNER_TEMP));
  assert.match(path.basename(root), /^dezirclip-surface-[a-f0-9]{32}$/);
  assert(!existsSync(root), 'Preserve any previous native run'); mkdirSync(root);
  const report = { schema_version: 1, kind: 'phase6_windows_native_functional', status: 'running', source_revision: expected.source_revision,
    run_id: env.GITHUB_RUN_ID, run_attempt: env.GITHUB_RUN_ATTEMPT, started_at: new Date().toISOString(), checks: [], observations: [],
    performance_scope: 'Observed show-to-native-visible latency and whole Desktop descendant process counters only; not first interactive frame, p50/p95, PSS/USS or a default-renderer performance gate.',
    limits: ['Actual production Desktop/Host/Surface using a fresh synthetic profile; no product installation.', 'No IME, multi-monitor, Linux, physical pointer dragging, accessibility conformance or helper update lifecycle claim.', 'Desktop descendant processes are terminated through retained original handles during fixture cleanup; natural application shutdown is not tested.'] };
  let native, desktop, bridge, movedHelper = null, sourceInputs = [], canary = null;
  const stage = async (name, action) => { const started = new Date().toISOString(); const evidence = await action(); report.checks.push({ name, status: 'passed', started_at: started, finished_at: new Date().toISOString(), evidence }); return evidence; };
  try {
    const destination = path.join(root, 'candidate'); mkdirSync(destination); mkdirSync(path.join(root, 'profile'));
    const desktopPath = path.join(destination, 'dezirclip.exe'), helper = path.join(destination, 'dzc-surface.exe');
    report.input_bytes = [copy(path.resolve(args.desktop), desktopPath), copy(path.resolve(args.helper), helper), copy(path.resolve(args['helper-proof']), path.join(root, 'helper.proof.json'))];
    const proof = json(args['helper-proof']);
    assert.equal(normalized(proof.executable), normalized(path.resolve(args.helper)));
    // Reject an old or unpatched helper before even its metadata-only execution.
    validateBuildProof(proof, proof.build_info, expected, regular(helper).sha256);
    sourceInputs = proof.inputs.map(item => {
      assert.equal(typeof item.path, 'string');
      assert(/^(?:frontends\/dzc-surface\/|clients\/go\/|design\/)/.test(item.path)
        || fixedSurfaceInputs.has(item.path), 'Unexpected Surface build input scope');
      const file = owned(path.resolve(repository, item.path), repository, true); assert.equal(regular(file).sha256, item.sha256); return { path: file, ...regular(file) };
    });
    const metadata = new LoggedProcess(helper, ['--build-info'], root, 'surface-build-info', environment(root));
    await completedCommand(metadata, 10000);
    const info = JSON.parse(metadata.stdout.toString('utf8').trim()); validateBuildProof(proof, info, expected, regular(helper).sha256);
    report.surface_build_info = info; report.helper_proof = { path: args['helper-proof'], ...regular(args['helper-proof']) };
    const canaryDirectory = path.join(destination, '.dzc-surface.old-' + randomUUID()); mkdirSync(canaryDirectory);
    const marker = Buffer.from('Phase6 owned startup canary. Preserve these complete bytes.\n');
    const markerFd = openSync(path.join(canaryDirectory, 'marker.txt'), 'wx');
    try { writeSync(markerFd, marker); fsyncSync(markerFd); } finally { closeSync(markerFd); }
    canary = canarySnapshot(canaryDirectory); report.startup_canary_before = canary;
    const port = await availablePort(); report.cdp_port = port;
    native = new NativeChannel(args.python, root, expected.source_revision, port);
    await Promise.race([native.ready, delay(20000).then(() => { throw new Error('Owned bootstrap did not become ready'); })]);
    const seedText = 'nativeupgradefixturemarker' + randomUUID().replaceAll('-', '') + '-native-a';
    const clipboard = new LoggedProcess(args.python, ['-B', fileURLToPath(new URL('./native-windows-clipboard.py', import.meta.url))], root, 'synthetic-clipboard-seed',
      { ...environment(root, true), DEZIRCLIP_NATIVE_CAPTURE_TEXT: seedText });
    await completedCommand(clipboard, 15000);
    await stage('independent_native_target_paste_calibration', async () => {
      const calibration = await native.request('target-preflight', { expected_text: seedText });
      assert.equal(calibration.actual_target_text, seedText);
      assert.equal(calibration.paste_attempts, 1);
      assert.equal(calibration.input.sent, 4);
      assert.equal(calibration.focus_before_input.foreground_hwnd, calibration.focus_before_input.target_hwnd);
      assert.equal(calibration.focus_before_input.own_thread_focus_hwnd, calibration.focus_before_input.edit_hwnd);
      assert.equal((await native.request('target-text')).text, '');
      return calibration;
    });
    const desktopEnv = { ...environment(root), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
      WEBVIEW2_USER_DATA_FOLDER: path.join(root, 'webview'), DEZIRCLIP_DB_PATH: path.join(root, 'profile/clipboard.db') };
    desktop = new LoggedProcess(desktopPath, [], root, 'desktop', desktopEnv);
    assert(Number.isSafeInteger(desktop.child.pid)); report.desktop_original = await native.request('bind', { pid: desktop.child.pid });
    bridge = new Bridge(); await bridge.connect(port, report.observations);
    const actualIdentity = await bridge.invoke('get_build_info');
    validateDesktopIdentity(actualIdentity, expected);
    const dataPath = await bridge.invoke('get_data_path'); assert.equal(normalized(dataPath), normalized(path.join(root, 'profile')));
    report.desktop_build_info_before = actualIdentity; report.actual_data_path_before = dataPath;
    await bridge.invoke('focus_clipboard_window');
    await bridge.invoke('set_idle_destroy_enabled', { enabled: false });
    await bridge.invoke('set_persistence', { enabled: true });
    await bridge.invoke('set_delete_after_paste', { enabled: false });
    await bridge.invoke('save_setting', { key: 'app.quick_paste_hotkey', value: 'Ctrl+Alt+Shift+F10' });
    await bridge.invoke('save_setting', { key: 'app.quick_paste_enabled', value: 'true' });
    await bridge.invoke('save_setting', { key: 'app.language', value: 'en' });
    const token = randomUUID().slice(0, 8).replace(/[0-9]/g, character => String.fromCharCode(97 + Number(character)));
    const query = 'alphaprobe' + token, body = `Native Surface ${query} complete synthetic payload`;
    const other = `Native Surface omegaprobe${token} other synthetic payload`;
    const id = await bridge.invoke('add_manual_item', { content: body, contentType: 'text', tags: [], expectedPrivacyEpoch: null, confirmPersistence: false });
    const otherId = await bridge.invoke('add_manual_item', { content: other, contentType: 'text', tags: [], expectedPrivacyEpoch: null, confirmPersistence: false });
    assert(Number.isSafeInteger(id) && id !== 0 && Number.isSafeInteger(otherId) && id !== otherId);
    report.synthetic_entries = [{ id, content: body }, { id: otherId, content: other }];
    await bridge.invoke('save_setting', { key: 'app.quick_paste_renderer', value: 'mygo' });
    await stage('mygo_releases_unused_tauri_webview', async () => {
      return poll('Unused Tauri Quick Paste WebView released', async () => {
        const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1500) });
        assert.equal(response.status, 200);
        const targets = await response.json(); assert(Array.isArray(targets));
        report.observations.push({ stage: 'CDP-unused-tauri-overlay', targets });
        assert.equal(mainWebviewTargets(targets).length, 1, 'The existing main WebView must remain alive');
        const quickPasteTargets = targets.filter(target => {
          try { return target.type === 'page' && new URL(target.url).searchParams.get('window') === 'quick-paste'; }
          catch { return false; }
        });
        return quickPasteTargets.length === 0 ? { targets, unused_quick_paste_webview_count: 0 } : null;
      });
    });
    let first;
    const activateAndShow = async (expectedPid = null) => {
      await native.request('activate-target'); const started = performance.now();
      await native.request('keys', { role: 'target', key: 'hotkey' });
      let attempts = 0;
      const evidence = await poll('Actual native Quick Paste', async () => {
        let status, observation;
        try {
          status = await bridge.invoke('get_quick_paste_surface_status'); observation = await native.request('observe');
          return { ...verifyPresented(status, observation, helper, expectedPid), status, observation };
        } catch (error) {
          assert(++attempts <= 128, 'Native show failure diagnostic bound exceeded');
          report.observations.push({ stage: 'native-show-pending', expected_pid: expectedPid, attempt: attempts,
            elapsed_ms: performance.now() - started, status, error: String(error.message ?? error).slice(0, 1024),
            helper_processes: observation?.processes.filter(item => normalized(item.image) === normalized(helper))
              .map(({ pid, running, exit_code, creation_time_100ns }) => ({ pid, running, exit_code, creation_time_100ns })),
            helper_windows: observation?.windows.filter(window => observation.processes.some(item => item.pid === window.pid
              && normalized(item.image) === normalized(helper))), foreground: observation?.foreground });
          return null;
        }
      });
      evidence.observed_hotkey_to_native_visible_ms = performance.now() - started;
      evidence.first_interactive_frame_measured = false; return evidence;
    };
    first = await stage('actual_hotkey_cold_native_show', () => activateAndShow());
    await stage('actual_escape_restores_target', async () => {
      await native.request('keys', { role: 'surface', key: 'escape' });
      const observation = await poll('Escape hide and actual target foreground', async () => {
        const status = await bridge.invoke('get_quick_paste_surface_status'), observed = await native.request('observe');
        return !status.visible && !observed.windows.some(w => w.pid === first.pid && w.visible)
          && observed.foreground.hwnd === observed.target_hwnd ? observed : null;
      });
      assert.equal((await native.request('target-text')).text, ''); return observation;
    });
    for (let cycle = 0; cycle < 2; cycle++) await stage('warm_show_hide_reuse_' + (cycle + 1), async () => {
      const shown = await activateAndShow(first.pid); assert.equal(shown.hwnd, first.hwnd); assert.equal(shown.session_id, first.session_id);
      assert.equal(shown.creation_time_100ns, first.creation_time_100ns);
      await native.request('keys', { role: 'surface', key: 'hotkey' });
      await poll('Actual hotkey hide', async () => !(await bridge.invoke('is_quick_paste_visible'))
        && !(await native.request('observe')).windows.some(w => w.pid === first.pid && w.visible));
      return shown;
    });
    await stage('external_blur_does_not_restore_previous_target', async () => {
      await activateAndShow(first.pid);
      const alternate = await native.request('activate-alternate');
      const result = await poll('External blur hides without taking focus back', async () => {
        const status = await bridge.invoke('get_quick_paste_surface_status'), observation = await native.request('observe');
        return !status.visible && !observation.windows.some(w => w.pid === first.pid && w.visible)
          && observation.foreground.hwnd === alternate.hwnd ? { status, observation } : null;
      });
      await delay(250);
      const delayed = await native.request('observe'); assert.equal(delayed.foreground.hwnd, alternate.hwnd);
      return { ...result, delayed, external_focus_preserved: true };
    });
    await stage('native_search_selection_and_real_target_paste', async () => {
      await native.request('clear-target'); await activateAndShow(first.pid);
      await native.request('keys', { role: 'surface', key: 'select-all' }); await native.request('keys', { role: 'surface', text: query });
      const accessibility = await poll('Actual accessible matching result', async () => {
        const result = await native.request('accessibility', { pid: first.pid });
        return result.elements.some(item => item.name === body && item.enabled && !item.offscreen)
          && result.elements.some(item => item.value === query)
          && !result.elements.some(item => item.name === other && !item.offscreen) ? result : null;
      }, 15000);
      const selectedAt = performance.now(); await native.request('keys', { role: 'surface', key: 'enter' });
      const pasteDeadline = performance.now() + 12000;
      let pasteAttempt = 0, pasteDiagnostics = 0;
      await poll('Actual selected text pasted into the native target', async () => {
        // The exact original target text remains the only success predicate.
        if ((await native.request('target-text')).text === body) return true;
        const attempt = ++pasteAttempt;
        if (pasteDiagnosticDue(attempt, pasteDiagnostics) && performance.now() < pasteDeadline) {
          const budget = Math.max(1, Math.min(200, pasteDeadline - performance.now()));
          const [status, observed] = await Promise.allSettled([
            bridge.invoke('get_quick_paste_surface_status', {}, budget),
            native.request('observe', {}, budget),
          ]);
          report.observations.push(pasteDiagnosticObservation(attempt, status, observed, first.pid));
          pasteDiagnostics++;
        }
        return false;
      }, 12000, pasteDeadline);
      const after = await native.request('observe'); assert.equal(after.foreground.hwnd, after.target_hwnd);
      assert.equal(await bridge.invoke('is_quick_paste_visible'), false);
      const history = await bridge.invoke('get_clipboard_history', { limit: 150, offset: 0, contentType: null });
      assert(history.some(item => item.id === id), 'Successful paste must retain the selected fixture record');
      return { entry_id: id, actual_target_text: body, accessibility, observation_after: after,
        selection_to_target_text_ms: performance.now() - selectedAt, native_paste_attempts: 1, stored_record_retained: true };
    });
    await stage('protected_content_is_unavailable_and_not_consumed', async () => {
      const previous = first;
      await bridge.invoke('focus_clipboard_window');
      // Opening main deliberately revokes the old Surface session. Warm reuse
      // applies inside a session, not across this explicit lifecycle boundary.
      const stopped = await poll('Main show revokes the original helper', async () => {
        const status = await bridge.invoke('get_quick_paste_surface_status');
        const observation = await native.request('observe');
        const process = observation.processes.find(item => item.pid === previous.pid);
        assert.equal(process?.creation_time_100ns, previous.creation_time_100ns);
        if (process.running || status.managed_pid !== null) return null;
        assert.equal(process.exit_code, 0);
        return { status, observation, original_process: process };
      });
      const protectedQuery = 'protectedprobe' + token;
      const protectedBody = `Synthetic protected Surface ${protectedQuery} payload`;
      const protectedId = await bridge.invoke('add_manual_item', { content: protectedBody, contentType: 'text', tags: ['sensitive'], expectedPrivacyEpoch: null, confirmPersistence: false });
      assert(Number.isSafeInteger(protectedId) && protectedId !== 0);
      const before = await bridge.invoke('get_clipboard_history', { limit: 150, offset: 0, contentType: null });
      assert(before.some(item => item.id === protectedId && item.tags?.includes('sensitive')));
      await native.request('clear-target'); first = await activateAndShow();
      assert.notEqual(first.session_id, previous.session_id, 'Main revocation must create a fresh Surface session');
      await native.request('keys', { role: 'surface', key: 'select-all' });
      await native.request('keys', { role: 'surface', text: protectedQuery });
      const accessibility = await poll('Actual restricted search has no protected result', async () => {
        const result = await native.request('accessibility', { pid: first.pid });
        assert(!result.elements.some(item => item.name.includes(protectedBody)), 'Protected body appeared in the native accessibility tree');
        return result.elements.some(item => item.value === protectedQuery)
          && result.elements.some(item => item.name === 'No results' && !item.offscreen)
          && !result.elements.some(item => [body, other].includes(item.name) && !item.offscreen) ? result : null;
      }, 15000);
      await native.request('keys', { role: 'surface', key: 'enter' });
      await delay(300);
      const status = await bridge.invoke('get_quick_paste_surface_status');
      assert.equal((await native.request('target-text')).text, '');
      const after = await bridge.invoke('get_clipboard_history', { limit: 150, offset: 0, contentType: null });
      assert(after.some(item => item.id === protectedId && item.tags?.includes('sensitive')));
      assert(after.some(item => item.id === id) && after.some(item => item.id === otherId));
      // Enter on an empty result deliberately leaves the panel open. End this
      // stage with a real Escape before the next stage sends a toggle hotkey.
      await native.request('keys', { role: 'surface', key: 'escape' });
      const dismissed = await poll('Restricted empty panel dismissed with Escape', async () => {
        const status = await bridge.invoke('get_quick_paste_surface_status');
        const observation = await native.request('observe');
        return !status.visible && !observation.windows.some(window => window.pid === first.pid && window.visible)
          && observation.foreground.hwnd === observation.target_hwnd ? { status, observation } : null;
      });
      return { entry_id: protectedId, actual_target_text: '', attempts: 1, status, accessibility,
        previous_session: previous, confirmed_previous_exit: stopped, new_session: first, dismissed,
        original_protected_record_retained: true, ordinary_fixture_records_retained: true,
        scope: 'Actual restricted UI search excludes the protected row and Enter cannot submit it. No forged protected-ID private frame or populated sequential queue is claimed.' };
    });
    await stage('real_privacy_epoch_revokes_native_display', async () => {
      await native.request('clear-target'); await activateAndShow(first.pid);
      const before = await bridge.invoke('get_clipboard_host_status');
      await bridge.invoke('set_privacy_session', { enabled: true, durationMs: null });
      const after = await bridge.invoke('get_clipboard_host_status');
      assert.equal(before.state.service_instance_id, after.state.service_instance_id);
      assert(after.state.privacy_epoch > before.state.privacy_epoch);
      const hidden = await poll('Native display revoked after actual epoch change', async () => {
        const status = await bridge.invoke('get_quick_paste_surface_status'), observation = await native.request('observe');
        return !status.visible && !observation.windows.some(w => w.pid === first.pid && w.visible) ? { status, observation } : null;
      });
      await native.request('activate-target'); await native.request('keys', { role: 'target', key: 'enter' }); await delay(300);
      assert.equal((await native.request('target-text')).text, '', 'Enter after revoked display must not paste the old selection');
      await bridge.invoke('set_privacy_session', { enabled: false, durationMs: null });
      return { before: before.state, after: after.state, hidden, stale_foreground_enter_pasted: false,
        boundary: 'Native user-observable revocation; not forged private-pipe stale-action injection.' };
    });
    await stage('switch_to_tauri_confirms_actual_helper_exit', async () => {
      const managedBeforeStop = await activateAndShow();
      await bridge.invoke('save_setting', { key: 'app.quick_paste_renderer', value: 'tauri' });
      const observation = await poll('Original helper exited after renderer switch', async () => {
        const value = await native.request('observe'); return value.processes.filter(p => normalized(p.image) === normalized(helper)).every(p => !p.running) ? value : null;
      });
      assert.equal(observation.processes.find(p => p.pid === managedBeforeStop.pid)?.exit_code, 0,
        'The actual normally stopped managed process must exit successfully');
      assert.equal((await bridge.invoke('get_quick_paste_surface_status')).managed_pid, null);
      await native.request('activate-target'); await native.request('keys', { role: 'target', key: 'hotkey' });
      const tauri = await poll('Actual restored Tauri Quick Paste', async () => {
        const visible = await bridge.invoke('is_quick_paste_visible');
        const observed = await native.request('observe');
        if (!visible) return null;
        return { ...observed, panel: verifyNativeTauriPanel(observed, desktop.child.pid) };
      });
      await bridge.invoke('hide_quick_paste');
      assert.deepEqual(canarySnapshot(canary.directory), canary);
      return { managed_before_stop: managedBeforeStop, original_helper_exit: observation, actual_tauri_window: tauri,
        startup_canary_preserved_after_actual_helper_run: true };
    });
    await stage('missing_owned_helper_falls_back_to_real_tauri', async () => {
      movedHelper = helper + '.qa-unavailable'; assert(!existsSync(movedHelper)); renameSync(helper, movedHelper);
      await bridge.invoke('save_setting', { key: 'app.quick_paste_renderer', value: 'mygo' });
      await native.request('activate-target'); await native.request('keys', { role: 'target', key: 'hotkey' });
      const result = await poll('Actual Tauri fallback', async () => {
        const status = await bridge.invoke('get_quick_paste_surface_status');
        const tauriVisible = await bridge.invoke('is_quick_paste_visible');
        const observation = await native.request('observe');
        if (status.managed_pid !== null || status.last_error !== 'renderer_unavailable' || !tauriVisible) return null;
        return { status, observation, panel: verifyNativeTauriPanel(observation, desktop.child.pid), tauri_window_visible: true };
      });
      await bridge.invoke('hide_quick_paste'); await bridge.invoke('save_setting', { key: 'app.quick_paste_renderer', value: 'tauri' });
      assert(!existsSync(helper)); renameSync(movedHelper, helper); movedHelper = null;
      assert.equal(regular(helper).sha256, proof.sha256); return result;
    });
    report.desktop_build_info_after = await bridge.invoke('get_build_info');
    assert.deepEqual(report.desktop_build_info_after, report.desktop_build_info_before);
    report.actual_data_path_after = await bridge.invoke('get_data_path'); assert.equal(normalized(report.actual_data_path_after), normalized(dataPath));
    report.managed_status_before_idle_destroy = await bridge.invoke('get_quick_paste_surface_status');
    assert.equal(report.managed_status_before_idle_destroy.managed_pid, null);
    await stage('main_idle_destroy_then_actual_native_cold_and_warm_show', async () => {
      await bridge.invoke('focus_clipboard_window');
      const before = await native.request('observe');
      const main = before.windows.filter(window => window.pid === desktop.child.pid && window.title === 'DezirClip');
      assert.equal(main.length, 1, 'Bind the actual original main HWND before idle destruction');
      await bridge.invoke('save_setting', { key: 'app.quick_paste_renderer', value: 'mygo' });
      await bridge.invoke('set_idle_destroy_seconds', { seconds: 5 });
      await bridge.invoke('set_idle_destroy_enabled', { enabled: true });
      await bridge.invoke('hide_window_cmd');
      const destroyed = await poll('Actual idle destruction of the original main HWND', async () => {
        const observed = await native.request('observe');
        try { verifyMainDestroyed(observed, desktop.child.pid, main[0].hwnd); return observed; } catch { return null; }
      }, 30000);
      // The main bridge is now gone. Observe only original native processes,
      // windows and foreground; no fabricated supervisor status substitutes.
      bridge.close();
      const activateWithoutMain = async (expectedPid = null) => {
        await native.request('activate-target'); await native.request('keys', { role: 'target', key: 'hotkey' });
        return poll('Actual native Surface while main is destroyed', async () => {
          const observed = await native.request('observe');
          try {
            verifyMainDestroyed(observed, desktop.child.pid, main[0].hwnd);
            return { ...verifyNativePanel(observed, helper, expectedPid), observation: observed };
          } catch { return null; }
        });
      };
      const escapeWithoutMain = async pid => {
        await native.request('keys', { role: 'surface', key: 'escape' });
        return poll('Native Escape hides and restores target without main', async () => {
          const observed = await native.request('observe');
          verifyMainDestroyed(observed, desktop.child.pid, main[0].hwnd);
          return !observed.windows.some(window => window.pid === pid && window.visible)
            && observed.foreground.hwnd === observed.target_hwnd ? observed : null;
        });
      };
      const cold = await activateWithoutMain();
      const coldHidden = await escapeWithoutMain(cold.pid);
      const warm = await activateWithoutMain(cold.pid);
      assert.equal(warm.hwnd, cold.hwnd); assert.equal(warm.creation_time_100ns, cold.creation_time_100ns);
      const warmHidden = await escapeWithoutMain(warm.pid);
      assert.deepEqual(canarySnapshot(canary.directory), canary);
      return { original_main: main[0], destroyed, cold, coldHidden, warm, warmHidden,
        inspection: 'Independent actual Win32 process/window/foreground observations after the main CDP bridge was destroyed.' };
    });
    assert(sourceInputs.every(item => regular(item.path).sha256 === item.sha256));
    for (const input of report.input_bytes) assert.deepEqual(regular(input.source), { bytes: input.bytes, sha256: input.sha256 });
    for (const input of report.input_bytes) assert.deepEqual(regular(input.path), { bytes: input.bytes, sha256: input.sha256 });
    report.status = 'passed';
    report.remaining_native_acceptance = ['Forged protected-ID private-frame injection; uncertain paste or native-target refusal with a populated sequential queue.',
      'IME composition, DPI/multiple monitors and update-time helper exit.', 'Repeatable same-function Tauri/MyGo cold/warm first-interactive-frame and whole-process performance comparison.'];
  } catch (error) {
    report.status = 'failed'; report.error = String(error.stack ?? error);
  } finally {
    const errors = [];
    if (bridge?.socket?.readyState === WebSocket.OPEN) {
      try { await bridge.invoke('hide_quick_paste'); await bridge.invoke('save_setting', { key: 'app.quick_paste_renderer', value: 'tauri' }); } catch (error) { errors.push(String(error)); }
      bridge.close();
    }
    if (native) {
      try {
        try { await native.request('stop'); } finally { native.child.stdin.end(); }
        report.native_target_exit = await Promise.race([native.closed, delay(20000).then(() => { throw new Error('Owned cleanup did not finish'); })]);
        assert.equal(report.native_target_exit.exit_code, 0); assert(!native.overflow && !native.error);
        report.cleanup = json(path.join(root, 'native-cleanup.json')); assert(!report.cleanup.native_error && !report.cleanup.bootstrap_error);
        assert.deepEqual(report.cleanup.native.remaining_owned_processes, []); assert.equal(report.cleanup.native.target_closed, true);
        assert.equal(report.cleanup.native.alternate_target_closed, true);
      } catch (error) { errors.push(String(error)); }
      finally { native.finish(); }
    }
    if (desktop) {
      try { report.desktop_exit = await Promise.race([desktop.closed, delay(5000).then(() => { throw new Error('Original Desktop still running'); })]); desktop.finish(); assert(!desktop.overflow && !desktop.error); }
      catch (error) { errors.push(String(error)); }
    }
    if (movedHelper) {
      try { const helper = path.join(root, 'candidate/dzc-surface.exe'); assert(!existsSync(helper)); renameSync(movedHelper, helper); report.unavailable_helper_bytes_restored = true; }
      catch (error) { errors.push(String(error)); }
    }
    try {
      const copies = (report.input_bytes ?? []).map(input => {
        assert.deepEqual(regular(input.source), { bytes: input.bytes, sha256: input.sha256 });
        assert.deepEqual(regular(input.path), { bytes: input.bytes, sha256: input.sha256 });
        return { source: input.source, path: input.path, sha256: input.sha256 };
      });
      assert(sourceInputs.every(input => regular(input.path).sha256 === input.sha256));
      report.input_integrity_after = { copied_originals: copies, helper_source_inputs_checked: sourceInputs.length, unchanged: true };
      if (canary) { report.startup_canary_after = canarySnapshot(canary.directory); assert.deepEqual(report.startup_canary_after, canary); }
    } catch (error) { errors.push('Input integrity: ' + String(error)); }
    report.cleanup_errors = errors; if (errors.length) report.status = 'failed';
    report.finished_at = new Date().toISOString();
    const reference = save(path.join(root, 'report.json'), report); console.log(JSON.stringify(reference));
  }
  assert.equal(report.status, 'passed', report.error ?? report.cleanup_errors.join('; '));
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const args = {};
  for (let index = 2; index < process.argv.length; index += 2) {
    const key = process.argv[index].slice(2); assert(['desktop', 'helper', 'helper-proof', 'python', 'output-root'].includes(key) && process.argv[index + 1]);
    assert(!Object.hasOwn(args, key)); args[key] = process.argv[index + 1];
  }
  assert(['desktop', 'helper', 'helper-proof', 'python', 'output-root'].every(key => args[key]));
  runSurfaceAcceptance(args).catch(error => { console.error(error.stack ?? error); process.exitCode = 1; });
}
