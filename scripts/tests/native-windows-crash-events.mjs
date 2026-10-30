import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import path from 'node:path';

const LIMIT = 64 * 1024;
const TIMEOUT = 10000;
const GUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const normalized = value => path.win32.normalize(value).toLowerCase();
const timestamp = value => typeof value === 'string' && Number.isFinite(Date.parse(value)) ? Date.parse(value) : NaN;
const integer = value => {
  if (typeof value !== 'string' || !/^(?:0x[0-9a-f]+|[0-9]+)$/i.test(value)) return null;
  try { return BigInt(value); } catch { return null; }
};
const reportId = value => {
  if (typeof value !== 'string') return null;
  const id = /^\{.*\}$/.test(value) ? value.slice(1, -1) : value;
  return GUID.test(id) && id !== '00000000-0000-0000-0000-000000000000' ? id.toLowerCase() : null;
};

// A cleanup termination is acceptable only when this exact PID is explicitly
// recorded as forced and its original child close occurred after cleanup began.
export function desktopExitVerdict(record, cleanupStartedAt, forcedPids = []) {
  const valid = Number.isSafeInteger(record?.pid) && record.pid > 0
    && Number.isInteger(record.exit_code) && record.exit_code >= -2147483648 && record.exit_code <= 4294967295
    && Number.isFinite(timestamp(record.closed_at)) && Number.isFinite(timestamp(cleanupStartedAt));
  const code = valid ? record.exit_code >>> 0 : null;
  const before = valid && timestamp(record.closed_at) < timestamp(cleanupStartedAt);
  const allowed = valid && !record.signal && (code === 0 || (code === 1 && !before && forcedPids.includes(record.pid)));
  return { allowed: Boolean(allowed), abnormal_before_cleanup: Boolean(valid && code !== 0 && before),
    exit_code_hex: code === null ? null : '0x' + code.toString(16).padStart(8, '0'),
    reason: !valid ? 'invalid_original_process_exit' : allowed ? (code === 0 ? 'normal_exit' : 'recorded_cleanup_termination') : 'abnormal_original_process_exit' };
}

function validateIdentity(identity) {
  assert(Number.isSafeInteger(identity.pid) && identity.pid > 0 && identity.pid <= 0xffffffff);
  const created = integer(identity.creation_time_100ns);
  assert(created !== null && created > 0n && created <= 0xffffffffffffffffn);
  assert.equal(identity.original_handle_retained, true);
  assert.equal(typeof identity.image, 'string');
  assert(path.win32.isAbsolute(identity.image));
  assert(Number.isFinite(timestamp(identity.started_at)) && Number.isFinite(timestamp(identity.closed_at)));
  assert(timestamp(identity.closed_at) >= timestamp(identity.started_at));
}

// Pure and intentionally strict: an executable basename or PID alone is never
// enough to attach an Application Error or WER event to this candidate.
export function selectOwnedCrashEvents(rows, identity) {
  validateIdentity(identity);
  assert(Array.isArray(rows) && rows.length <= 64);
  const start = timestamp(identity.started_at), end = timestamp(identity.closed_at) + 5000;
  const within = row => timestamp(row.time_created) >= start && timestamp(row.time_created) <= end;
  const matched = [], reports = new Set();
  let unbound = 0;
  for (const row of rows) {
    if (row.id !== 1000 || row.provider !== 'Application Error' || !within(row)) continue;
    const fields = row.fields ?? {};
    if (typeof fields.AppPath !== 'string' || !path.win32.isAbsolute(fields.AppPath) || normalized(fields.AppPath) !== normalized(identity.image)
      || integer(fields.ProcessId) !== BigInt(identity.pid)) continue;
    if (integer(fields.ProcessCreationTime) !== integer(identity.creation_time_100ns)) { unbound++; continue; }
    const id = reportId(fields.IntegratorReportId ?? fields.ReportId);
    if (id) reports.add(id);
    matched.push({ id: 1000, provider: row.provider, time_created: row.time_created, record_id: row.record_id,
      pid: identity.pid, creation_time_100ns: identity.creation_time_100ns, application_path: identity.image,
      module_name: fields.ModuleName ?? null, module_path: fields.ModulePath ?? null,
      exception_code: fields.ExceptionCode ?? null, faulting_offset: fields.FaultingOffset ?? null, report_id: id });
  }
  for (const row of rows) {
    if (row.id !== 1001 || row.provider !== 'Windows Error Reporting' || !within(row)) continue;
    const id = reportId(row.fields?.ReportId ?? row.fields?.IntegratorReportId);
    if (id && reports.has(id)) matched.push({ id: 1001, provider: row.provider, time_created: row.time_created,
      record_id: row.record_id, report_id: id, association: 'exact_report_id_from_bound_application_error' });
  }
  return { status: matched.length ? 'matched' : unbound ? 'unbound' : 'missing', events: matched, unbound_count: unbound };
}

export function validateCrashEventContext(identity, env, platform) {
  validateIdentity(identity);
  assert.equal(platform, 'win32');
  assert.equal(env.GITHUB_ACTIONS, 'true');
  assert.equal(env.RUNNER_ENVIRONMENT, 'github-hosted');
  assert.equal(env.GITHUB_REPOSITORY, 'qiyuanhuakai/dezirclip');
  assert.match(env.GITHUB_SHA ?? '', /^[0-9a-f]{40}$/);
  for (const key of ['GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT']) assert.match(env[key] ?? '', /^[1-9][0-9]*$/);
  assert(['push', 'pull_request', 'workflow_dispatch'].includes(env.GITHUB_EVENT_NAME));
  assert(path.win32.isAbsolute(env.RUNNER_TEMP ?? ''));
  const relative = path.win32.relative(normalized(env.RUNNER_TEMP), normalized(identity.image));
  assert.match(relative, /^(?:dezirclip-performance-[a-f0-9]{32}\\)?dezirclip-surface-[a-f0-9]{32}\\candidate\\dezirclip\.exe$/);
  assert(path.win32.isAbsolute(env.SystemRoot ?? env.SYSTEMROOT ?? ''));
}

// No registry changes, debug privileges, dumps, process attachment, retries, or
// machine-wide log export. Only already-written events for the owned child.
const script = String.raw`
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$request = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:DZC_OWNED_CRASH_REQUEST)) | ConvertFrom-Json
$start = [DateTimeOffset]::Parse($request.started_at).UtcDateTime
$end = [DateTimeOffset]::Parse($request.closed_at).UtcDateTime.AddSeconds(5)
function NumberValue($value) {
  try {
    if ($value -match '^0x[0-9a-fA-F]+$') { return [Convert]::ToUInt64($value.Substring(2),16) }
    if ($value -match '^[0-9]+$') { return [UInt64]::Parse($value) }
  } catch { return $null }
  return $null
}
try {
  $events = @(Get-WinEvent -FilterHashtable @{ LogName='Application'; ProviderName=@('Application Error','Windows Error Reporting'); Id=@(1000,1001); StartTime=$start; EndTime=$end } -MaxEvents 64 -ErrorAction Stop)
} catch {
  if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') { $events=@() } else { throw }
}
$bound = @(); $wer = @(); $ids = @(); $unbound = 0
foreach ($event in $events) {
  $xml = [xml]$event.ToXml(); $fields = @{}
  foreach ($data in $xml.Event.EventData.Data) { if ($data.Name) { $fields[[string]$data.Name] = [string]$data.'#text' } }
  if ($event.Id -eq 1000 -and $event.ProviderName -eq 'Application Error') {
    if (-not $fields.AppPath -or -not [IO.Path]::IsPathRooted($fields.AppPath)) { continue }
    try { $eventPath=[IO.Path]::GetFullPath($fields.AppPath) } catch { continue }
    if (-not [string]::Equals($eventPath,[IO.Path]::GetFullPath([string]$request.image),[StringComparison]::OrdinalIgnoreCase)) { continue }
    if ((NumberValue $fields.ProcessId) -ne [UInt64]$request.pid) { continue }
    if ((NumberValue $fields.ProcessCreationTime) -ne (NumberValue $request.creation_time_100ns)) { $unbound++; continue }
    $selected = @{}
    foreach ($name in @('AppPath','ProcessId','ProcessCreationTime','ModuleName','ModulePath','ExceptionCode','FaultingOffset','IntegratorReportId','ReportId')) {
      if ($fields.ContainsKey($name)) { if ($fields[$name].Length -gt 2048) { throw 'Owned event field exceeds bound' }; $selected[$name]=$fields[$name] }
    }
    $bound += @{ id=1000; provider=$event.ProviderName; time_created=$event.TimeCreated.ToUniversalTime().ToString('o'); record_id=[string]$event.RecordId; fields=$selected }
    foreach ($name in @('IntegratorReportId','ReportId')) { if ($fields[$name]) { $ids += $fields[$name].Trim('{}').ToLowerInvariant() } }
  } elseif ($event.Id -eq 1001 -and $event.ProviderName -eq 'Windows Error Reporting') {
    $wer += @{ id=1001; provider=$event.ProviderName; time_created=$event.TimeCreated.ToUniversalTime().ToString('o'); record_id=[string]$event.RecordId; fields=@{ ReportId=$fields.ReportId; IntegratorReportId=$fields.IntegratorReportId } }
  }
}
foreach ($event in $wer) {
  $id = $event.fields.ReportId; if (-not $id) { $id=$event.fields.IntegratorReportId }
  if ($id -and $ids -contains $id.Trim('{}').ToLowerInvariant()) { $bound += $event }
}
@{ rows=@($bound); queried_count=$events.Count; unbound_count=$unbound } | ConvertTo-Json -Depth 6 -Compress
`;

export async function collectOwnedCrashEvents(identity, exitRecord, cleanupStartedAt, env = process.env, platform = process.platform) {
  const verdict = desktopExitVerdict(exitRecord, cleanupStartedAt);
  if (!verdict.abnormal_before_cleanup) return { status: 'not_applicable', verdict, events: [] };
  try {
    validateCrashEventContext(identity, env, platform);
    assert.equal(identity.pid, exitRecord.pid);
    assert.equal(normalized(identity.image), normalized(exitRecord.executable));
    assert.equal(identity.started_at, exitRecord.started_at);
    assert.equal(identity.closed_at, exitRecord.closed_at);
  }
  catch { return { status: 'unavailable', reason: 'owned_github_context_not_verified', verdict, events: [] }; }
  const systemRoot = env.SystemRoot ?? env.SYSTEMROOT;
  const executable = path.win32.join(systemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
  const childEnv = Object.fromEntries(Object.entries(env).filter(([key]) => ['SYSTEMROOT', 'WINDIR', 'PATH', 'TEMP', 'TMP'].includes(key.toUpperCase())));
  childEnv.DZC_OWNED_CRASH_REQUEST = Buffer.from(JSON.stringify(identity), 'utf8').toString('base64');
  return new Promise(resolve => {
    let done = false, output = Buffer.alloc(0), bytes = 0, readTimer, deadlineTimer, closeTimer, stopReason;
    const reader = { pid: null, started_at: new Date().toISOString(), closed_at: null,
      exit_code: null, signal: null, kill_requested: false, kill_accepted: null, termination_confirmed: false };
    const finish = value => {
      if (done) return;
      done = true;
      clearTimeout(readTimer); clearTimeout(deadlineTimer); clearTimeout(closeTimer);
      resolve({ ...value, verdict, reader: { ...reader } });
    };
    let child;
    try { child = spawn(executable, ['-NoProfile', '-NonInteractive', '-Command', script],
      { env: childEnv, windowsHide: true, shell: false, stdio: ['ignore', 'pipe', 'pipe'] }); }
    catch { finish({ status: 'unavailable', reason: 'event_reader_spawn_failed', events: [] }); return; }
    reader.pid = child.pid ?? null;
    const unconfirmed = () => {
      if (done) return;
      // Do not let a reader that ignored termination hold the controller open.
      // Its original PID and unconfirmed status remain explicit in the receipt.
      child.stdout.destroy(); child.stderr.destroy(); child.unref();
      finish({ status: 'unavailable', reason: 'event_reader_termination_unconfirmed', stop_reason: stopReason, events: [] });
    };
    const stop = reason => {
      if (done || stopReason) return;
      stopReason = reason;
      clearTimeout(readTimer);
      reader.kill_requested = true;
      try { reader.kill_accepted = child.kill(); } catch { reader.kill_accepted = false; }
      closeTimer = setTimeout(unconfirmed, 2000);
    };
    // Reserve the final two seconds of the total bound for original-child close
    // acknowledgement. A successful kill request alone is not cleanup proof.
    readTimer = setTimeout(() => stop('event_reader_timeout'), TIMEOUT - 2000);
    deadlineTimer = setTimeout(() => { stop('event_reader_timeout'); unconfirmed(); }, TIMEOUT);
    for (const [name, stream] of [['stdout', child.stdout], ['stderr', child.stderr]]) stream.on('data', chunk => {
      if (done || stopReason) return;
      bytes += chunk.length;
      if (bytes > LIMIT) { stop('event_reader_output_limit'); return; }
      if (name === 'stdout') output = Buffer.concat([output, chunk]);
    });
    child.once('error', () => {
      if (reader.pid) stop('event_reader_failed');
      else finish({ status: 'unavailable', reason: 'event_reader_spawn_failed', events: [] });
    });
    child.once('close', (code, signal) => {
      if (done) return;
      Object.assign(reader, { closed_at: new Date().toISOString(), exit_code: code, signal, termination_confirmed: true });
      if (stopReason) { finish({ status: 'unavailable', reason: stopReason, events: [] }); return; }
      if (code !== 0) { finish({ status: 'unavailable', reason: 'event_reader_nonzero_exit', events: [] }); return; }
      try {
        const result = JSON.parse(output.toString('utf8').replace(/^\uFEFF/, ''));
        const selected = selectOwnedCrashEvents(result.rows, identity);
        const unbound = Number.isSafeInteger(result.unbound_count) ? result.unbound_count : 0;
        finish({ ...selected, status: selected.status === 'missing' && unbound > 0 ? 'unbound' : selected.status,
          unbound_count: selected.unbound_count + unbound, queried_count: result.queried_count ?? null,
          evidence: 'bounded_read_only_application_events' });
      } catch { finish({ status: 'unavailable', reason: 'event_reader_invalid_result', events: [] }); }
    });
  });
}
