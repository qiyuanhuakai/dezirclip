"""Read a frozen process-only ZIP and recompute its records; never extract or execute it."""
import collections
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import sys
import zipfile

SOURCE = '1fe685c424602b2bc1892f1deb0794c2e0b90ad5'
METHOD = 'process-resource-only-v2'
COLLECTION = 'not_collected'
SUMMARY_SHA = '3c9e9b1d8162859efc11aa5a30cede300b5bf77418559c52f5cb389367807b86'
MANIFEST_SHA = '63d0f6f062a5be89db5c534e72ca175677c3a6be6492655879acdcacebb7f30f'
ROOT_FINAL_SHA = '1c03cb4c3654df48f329d974f03cb9da1151a98a7d36c411e859a8eb99ceaad9'
READER_SHA = 'd32cf419ae5e90bde54ac673d33023aaa8cfd1fed5559c63d3992ee29d0e043e'
READER_FREEZE_SHA = '8078f18ed9f13846fef7e71bfadc508a431817e3c04d4be4fc73d22d0b177f25'
AUDIT_SHA = '39044e15012b6e4a31e9a97af33fa8260a0edbf456e05225144f2e0ed0b4e04c'
AUDIT_SOURCE_SHA = 'cb270b6ed32cbc264c21f79f0a1c643e94522882cdfa25d5d06fc4f76824ec1c'
COHORT_CONTROL_SHA = 'ddc90fb94e0fc660f096198394ddaa97ac0ffcbd18b5231a4ece5040c706fcb0'
ARMS = tuple({'arm_index': i, 'renderer': r, 'run_name': f'process-v2-1fe685c-{i}-{r}'}
             for i, r in enumerate(('tauri', 'mygo', 'mygo', 'tauri'), 1))
REVIEWS = ('32185a1f7beb4652fd0643ddb320f76f6f4de653ae973ffa6f4fb034ce855725',
           '48724c969aca2c688336a8108d3fca2e219cd6c3a46260e0f8bac86ebe1ab94e',
           '5f19851e74bab8f0876645a8b844ba3dc7fa1248fe4178455835975472f8e94e',
           '7201987fdc7df2a840e3fb0268493586c398277225d99ff14d7e2e1209f2119c')
RAW_NAMES = ('credential-boundary.json', 'functional/keeper.json', 'functional/keeper.stderr.log',
    'functional/keeper.stdout.log', 'functional/native-observations.jsonl', 'functional/paste-tool-version.stderr.log',
    'functional/paste-tool-version.stdout.log', 'functional/performance.json', 'functional/private-ss/proof.json',
    'functional/resource-clock-hidden.jsonl', 'functional/resource-clock-visible.jsonl',
    'functional/resource-visible-query.png', 'functional/resource-visible-query.stderr.log',
    'functional/resource-visible-query.stdout.log', 'functional/resources-hidden.jsonl',
    'functional/resources-visible.jsonl', 'functional/show-status-qualification.jsonl',
    'functional/surface-before-warmup.png', 'functional/surface-before-warmup.stderr.log',
    'functional/surface-before-warmup.stdout.log', 'functional/surface-build-info.stderr',
    'functional/surface-build-info.stdout', 'functional/target.stderr.log', 'functional/target.stdout.jsonl',
    'functional/webdriver.log', 'functional/wm.stderr.log', 'functional/wm.stdout.log', 'host-load.jsonl',
    'host-preflight.json', 'outer.stderr.log', 'outer.stdout.log', 'plan.json', 'receipt.json', 'runtime.json')
COHORT_GLOBAL = ('comparison-count-v1.plan.json', 'preparation.json', 'prepared-only-note.json',
                 'integration-manifest.json', 'complete-process-only-v2.root-summary-review.json',
                 'comparison-process-only-v2.limited-descriptive.json')
COHORT_SUFFIXES = ('assessment.json', 'authorization.json', 'execution.json', 'independent-raw-review.json',
    'pending.json', 'postflight-windows.json', 'preflight-linux.json', 'preflight-windows.json',
    'root-once-review.json', 'root-png.json', 'scheduling.json', 'setup-copy-proof.json', 'setup.png')
COHORT_NAMES = frozenset(COHORT_GLOBAL) | {a['run_name'] + '.' + suffix for a in ARMS for suffix in COHORT_SUFFIXES}
OMITTED = {'desktop', 'helper', 'private_ss_helper', 'webkit_driver', 'python', 'screenshot_tool', 'paste_tool'}
RAW_ORIGINAL = '/home/qiyuaner/.cache/dezirclip-v02/phase6-performance-recovered/' + SOURCE
MAX_MEMBER = 96 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024
MAX_JSON = 4 * 1024 * 1024
CHECKS = {'original_bytes', 'all_raw_evidence', 'input_and_source_identity', 'six_namespaces',
          'native_focus_and_paste', 'qualifications_and_replies', 'original_process_identities',
          'all_capture_rows_and_clocks', 'process_counter_math', 'thread_metrics_not_collected',
          'root_actual_png_view', 'private_pidfd_keeper_secret_service_cleanup', 'unverified_host_metadata_limits'}
FIELDS = {'kind', 'status', 'source_revision', 'observer_method', 'thread_context_switch_collection',
          'arm', 'renderer', 'arm_index', 'reviewer_role', 'independent_from_authorizer_and_assessor_author',
          'actual_original_raw_review_completed', 'whole_ABBA_complete', 'checks', 'root_assessment',
          'root_png_evidence', 'setup_copy_proof', 'assessor', 'integration_manifest', 'reviewer_reader',
          'execution', 'raw_evidence', 'whole_host_idle', 'quiet_window', 'performance_acceptance',
          'performance_eligible', 'phase6_passed', 'phase7_authorized', 'next_arm_authorized'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def summary(values):
    values = sorted(values)
    require(values and all(type(v) in (int, float) and math.isfinite(v) for v in values), 'Invalid numeric vector')
    return {'count': len(values), 'min': values[0], 'max': values[-1], 'mean': sum(values) / len(values),
            'p50': values[math.ceil(len(values) * .5) - 1], 'p95': values[math.ceil(len(values) * .95) - 1]}


def equal(actual, expected, path='value'):
    if isinstance(expected, dict):
        require(isinstance(actual, dict) and set(actual) == set(expected), path + ': object keys differ')
        for key in expected:
            equal(actual[key], expected[key], path + '.' + key)
    elif isinstance(expected, list):
        require(isinstance(actual, list) and len(actual) == len(expected), path + ': array differs')
        for index, value in enumerate(expected):
            equal(actual[index], value, path + '[' + str(index) + ']')
    elif type(expected) is int:
        require(type(actual) is int and actual == expected, path + ': integer differs')
    elif type(expected) is float:
        require(type(actual) in (int, float) and math.isfinite(actual) and math.isclose(actual, expected, rel_tol=1e-12, abs_tol=1e-9), path + ': number differs')
    else:
        require(type(actual) is type(expected) and actual == expected, path + ': value differs')


def parse(data):
    def unique(items):
        result = {}
        for key, value in items:
            require(key not in result, 'Duplicate JSON key')
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda v: (_ for _ in ()).throw(ValueError('Nonfinite JSON: ' + v)))


def method(value):
    require(value['observer_method'] == METHOD and value['thread_context_switch_collection'] == COLLECTION, 'Process-only method differs')


def scope(value):
    for key in ('performance_eligible', 'phase6_passed', 'phase7_authorized', 'next_arm_authorized'):
        require(value[key] is False, 'Unqualified acceptance claim: ' + key)
    for key in ('whole_host_idle', 'quiet_window', 'performance_acceptance'):
        require(value[key] is None, 'Unknown host/performance claim: ' + key)


def safe_name(name):
    if type(name) is not str or len(name) > 240:
        return False
    path = PurePosixPath(name)
    return bool(re.fullmatch('[A-Za-z0-9._/-]+', name)) and not path.is_absolute() and '..' not in path.parts and str(path) == name and not name.endswith('/')


def original_name(value):
    require(type(value) is str and 0 < len(value) <= 2048 and not any(ord(c) < 32 for c in value) and
            (value.startswith('/') or re.match('[A-Za-z]:[\\\\/]', value)) and
            '..' not in value.replace('\\', '/').split('/'), 'Invalid original-path label')


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True, allow_nan=False).encode('ascii')


def elf_header(data):
    require(type(data) is bytes and len(data) == 64 and data[:4] == b'\x7fELF' and
            data[4:7] == b'\x02\x01\x01', 'Recorded header must be native little-endian ELF64')
    kind, machine, version = struct.unpack_from('<HHI', data, 16)
    require(kind in (2, 3) and machine == 62 and version == 1, 'Recorded ELF type/machine/version differs')
    return {'header_hex': data.hex(), 'class': 'ELF64', 'byte_order': 'little', 'machine': machine,
            'type': kind, 'full_bytes_included': False, 'public_full_hash_rechecked': False}


class Bundle:
    def __init__(self, archive):
        self.archive, self.used = archive, set()
        entries = archive.infolist()
        names = [v.filename for v in entries]
        require(len(entries) <= 320 and len(names) == len(set(names)) == len(set(n.casefold() for n in names)) and
                all(safe_name(n) for n in names), 'Unsafe, duplicate or excessive ZIP inventory')
        require(all(not (v.flag_bits & 1) and stat.S_IFMT(v.external_attr >> 16) in (0, stat.S_IFREG)
                    and v.file_size <= MAX_MEMBER for v in entries) and
                sum(v.file_size for v in entries) <= MAX_TOTAL, 'Encrypted, special or oversized ZIP member')
        require('manifest.json' in names and archive.getinfo('manifest.json').file_size <= 2 * 1024 * 1024, 'Manifest missing or oversized')
        self.manifest = parse(archive.read('manifest.json'))
        m = self.manifest
        require(type(m) is dict and set(m) == {'schema_version', 'kind', 'source_revision', 'observer_method',
            'thread_context_switch_collection', 'summary', 'integration_manifest', 'reader', 'reader_freeze',
            'root_final_review', 'independent_summary_audit', 'independent_summary_source', 'public_sources',
            'files', 'omitted_elf_inputs', 'performance_eligible', 'phase6_passed', 'phase7_authorized',
            'next_arm_authorized', 'whole_host_idle', 'quiet_window', 'performance_acceptance', 'limits'},
            'Exact public manifest schema required')
        require(type(m['schema_version']) is int and m['schema_version'] == 1 and
                m['kind'] == 'phase6_linux_process_only_v2_public_analysis_inputs' and m['source_revision'] == SOURCE,
                'Wrong frozen source/bundle')
        method(m)
        scope(m)
        require(type(m['files']) is list and len(m['files']) <= 319, 'Closed file inventory required')
        self.originals, self.members = {}, {}
        for item in m['files']:
            require(type(item) is dict and set(item) == {'original_paths', 'archive_path', 'role', 'bytes', 'sha256'} and
                    item['role'] in ('raw', 'cohort', 'source', 'proof') and safe_name(item['archive_path']) and
                    type(item['bytes']) is int and 0 <= item['bytes'] <= MAX_MEMBER and type(item['sha256']) is str and
                    re.fullmatch('[0-9a-f]{64}', item['sha256']) and type(item['original_paths']) is list and
                    1 <= len(item['original_paths']) <= 8, 'Invalid explicit file map')
            require(item['archive_path'] not in self.members, 'Duplicate member mapping')
            self.members[item['archive_path']] = item
            data = archive.read(item['archive_path'])
            require(len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256'], 'Original member bytes/SHA differ')
            require(data[:4] != b'\x7fELF' and data[:2] != b'MZ' and not data.startswith(b'SQLite format 3\x00'), 'Executable/database payload is excluded')
            for name in item['original_paths']:
                original_name(name)
                require(name not in self.originals, 'Duplicate original-path alias')
                self.originals[name] = item
        require(set(names) == {'manifest.json'} | set(self.members), 'Extra or missing ZIP member')
        expected_raw = {'raw/' + a['run_name'] + '/' + n for a in ARMS for n in RAW_NAMES}
        require({n for n, v in self.members.items() if v['role'] == 'raw'} == expected_raw, 'Exactly four closed34 raw inventories required')
        cohort = {n.removeprefix('cohort/'): {'bytes': v['bytes'], 'sha256': v['sha256']}
                  for n, v in self.members.items() if v['role'] == 'cohort' and n.startswith('cohort/')}
        require(set(cohort) == COHORT_NAMES and sum(v['role'] == 'cohort' for v in self.members.values()) == 58 and
                hashlib.sha256(canonical(cohort)).hexdigest() == COHORT_CONTROL_SHA, 'Original 58-file cohort control differs')

    def item(self, reference, role=None):
        name = reference if type(reference) is str else reference['path']
        require(name in self.originals, 'Required original is not included: ' + str(name))
        item = self.originals[name]
        if type(reference) is not str:
            require(type(reference) is dict and set(reference) == {'path', 'bytes', 'sha256'} and
                    type(reference['bytes']) is int and {k: item[k] for k in ('bytes', 'sha256')} ==
                    {k: reference[k] for k in ('bytes', 'sha256')}, 'Original reference differs from included bytes')
        require(role is None or item['role'] == role, 'Original file role differs')
        self.used.add(item['archive_path'])
        return item

    def data(self, reference, limit=MAX_MEMBER):
        item = self.item(reference)
        require(item['bytes'] <= limit, 'Original exceeds this reader bound')
        return self.archive.read(item['archive_path'])

    def json(self, reference):
        return parse(self.data(reference, MAX_JSON))

    def lines(self, reference):
        data = self.data(reference)
        require(data.endswith(b'\n'), 'Truncated JSONL')
        values = [parse(line) for line in data.splitlines()]
        require(values and len(values) <= 100000, 'Empty or excessive JSONL')
        return values

    def at(self, archive_path):
        require(archive_path in self.members, 'Fixed member is missing')
        item = self.members[archive_path]
        self.used.add(archive_path)
        return item['original_paths'][0]

    def ref(self, original):
        item = self.item(original)
        return {'path': str(original), 'bytes': item['bytes'], 'sha256': item['sha256']}


def check_resource(rows, clocks, progress, metric, main, panel):
    require(all(type(progress[k]) is int for k in ('requested_count', 'capture_calls', 'samples_written', 'interval_ns', 'deadline_ns', 'started_ns', 'tail_due_ns', 'tail_reached_ns', 'files_flushed_ns')), 'Progress integers required')
    require(len(rows) == len(clocks) == len(progress['clocks']) == 960, 'Resource/clock count differs')
    require(progress['requested_count'] == progress['capture_calls'] == progress['samples_written'] == 960, 'Resource capture accounting differs')
    require(progress['kind'] == 'event-anchored-count-v1' and progress['status'] == 'passed' and metric['sampling_protocol'] == 'event-anchored-count-v1' and type(metric['sample_count']) is int and metric['sample_count'] == 960, 'Resource protocol differs')
    require(progress['interval_ns'] == 250000000 and 0 < progress['deadline_ns'] - progress['started_ns'] <= 1260000000000, 'Resource interval/deadline differs')
    method(metric)
    for key in ('stable_thread_membership', 'observed_thread_context_switch_delta', 'wakeup_count'):
        require(metric[key] is None, 'Uncollected thread metric must be null')
    families, previous = [], None
    clock_keys = {'index', 'due_ns', 'actual_start_ns', 'late_ns', 'capture_end_ns', 'serialization_end_ns', 'sample_write_end_ns', 'clock_log_end_ns'}
    for index, (row, clock, saved) in enumerate(zip(rows, clocks, progress['clocks']), 1):
        require(set(saved) == clock_keys and all(type(v) is int for v in saved.values()), 'Clock schema differs')
        equal(clock, {key: value for key, value in saved.items() if key != 'clock_log_end_ns'}, 'raw clock')
        equal(row['sampling_clock'], {key: saved[key] for key in ('index', 'due_ns', 'actual_start_ns', 'late_ns', 'capture_end_ns')})
        require(type(row['snapshot_index']) is int and type(row['monotonic_ns']) is int, 'Sample integer clock required')
        require(saved['index'] == row['snapshot_index'] == index and row['monotonic_ns'] == saved['actual_start_ns'], 'Sample index/time differs')
        require(saved['due_ns'] == (previous['actual_start_ns'] + 250000000 if previous else progress['started_ns']), 'Pacing differs')
        require(saved['late_ns'] == saved['actual_start_ns'] - saved['due_ns'] >= 0, 'Lateness differs')
        require(saved['actual_start_ns'] <= saved['capture_end_ns'] <= saved['serialization_end_ns'] <= saved['sample_write_end_ns'] <= saved['clock_log_end_ns'] < progress['deadline_ns'], 'Clock ordering differs')
        require(type(row['observer_cost_ns']) is int and 0 <= row['observer_cost_ns'] <= saved['capture_end_ns'] - saved['actual_start_ns'], 'Observer cost differs')
        bounds = row['cpu_counter_read_window_ns']
        require(len(bounds) == 2 and all(type(v) is int for v in bounds) and saved['actual_start_ns'] <= bounds[0] <= bounds[1] <= saved['capture_end_ns'], 'CPU read window differs')
        if previous:
            require(saved['actual_start_ns'] >= previous['clock_log_end_ns'], 'Capture overlaps prior clock write')
        previous = saved
        method(row)
        require(row['context_switches'] is None, 'Uncollected context switches must be null')
        members = row['members']
        require(type(members) is list and members and len({m['pid'] for m in members}) == len(members), 'Duplicate/empty process family')
        family = {(m['pid'], m['started']) for m in members}
        require({(main['pid'], main['started']), (panel['pid'], panel['started'])} <= family, 'Original process missing')
        for member in members:
            method(member)
            require(member['threads'] is None, 'Uncollected threads must be null')
            require(all(type(member[k]) is int and member[k] > 0 for k in ('pid', 'started')), 'Invalid process identity')
            require(type(member['cpu_ticks']) is int and member['cpu_ticks'] >= 0, 'Invalid CPU counter')
            memory = member['memory_kib']
            require({'Pss', 'Rss', 'Private_Clean', 'Private_Dirty'} <= set(memory) and all(type(v) is int and v >= 0 for v in memory.values()), 'Invalid memory counters')
        equal(row['pss_kib'], sum(m['memory_kib']['Pss'] for m in members), 'PSS sum')
        equal(row['uss_kib'], sum(sum(m['memory_kib'].get(k, 0) for k in ('Private_Clean', 'Private_Dirty', 'Private_Hugetlb')) for m in members), 'USS sum')
        equal(row['cpu_ticks'], sum(m['cpu_ticks'] for m in members), 'CPU sum')
        families.append(family)
        for side in ('original_identities_before', 'original_identities_after'):
            for role, identity in (('main', main), ('panel', panel)):
                require(all(row[side][role][k] == identity[k] for k in ('pid', 'started')), 'Retained identity differs')
    require(progress['tail_due_ns'] == previous['actual_start_ns'] + 250000000, 'Tail deadline differs')
    require(max(progress['tail_due_ns'], previous['clock_log_end_ns']) <= progress['tail_reached_ns'] <= progress['files_flushed_ns'] < progress['deadline_ns'], 'Tail/flush ordering differs')
    require(progress['files_flushed_ns'] - progress['started_ns'] >= 240000000000 and rows[-1]['monotonic_ns'] - rows[0]['monotonic_ns'] >= 239750000000, 'Capture duration differs')
    equal(metric['actual_elapsed_seconds'], (progress['files_flushed_ns'] - progress['started_ns']) / 1000000000.0)
    equal(metric['sample_span_seconds'], (rows[-1]['monotonic_ns'] - rows[0]['monotonic_ns']) / 1000000000.0)
    for key in ('pss_kib', 'uss_kib'):
        equal(metric[key], summary([row[key] for row in rows]), key)
    equal(metric['observer_cost_ms'], summary([row['observer_cost_ns'] / 1000000.0 for row in rows]))
    equal(metric['late_ms'], summary([clock['late_ns'] / 1000000.0 for clock in progress['clocks']]))
    for name, start, end in (('serialization_cost_ms', 'capture_end_ns', 'serialization_end_ns'), ('sample_write_cost_ms', 'serialization_end_ns', 'sample_write_end_ns'), ('clock_log_cost_ms', 'sample_write_end_ns', 'clock_log_end_ns')):
        equal(metric[name], summary([(c[end] - c[start]) / 1000000.0 for c in progress['clocks']]))
    equal(metric['observation_intervals_ms'], summary([(b['monotonic_ns'] - a['monotonic_ns']) / 1000000.0 for a, b in zip(rows, rows[1:])]))
    stable = all(family == families[0] for family in families)
    require(metric['stable_process_membership'] is stable, 'Process stability differs')
    if not stable:
        require(metric['cpu_counter_normalization'] is None and metric['cpu_percent_one_core'] is None, 'Changing process family CPU must be unknown')
        return
    first, last = rows[0]['cpu_counter_read_window_ns'], rows[-1]['cpu_counter_read_window_ns']
    delta, hz = rows[-1]['cpu_ticks'] - rows[0]['cpu_ticks'], metric['cpu_clock_ticks_per_second']
    low, high, twice = last[0] - first[1], last[1] - first[0], sum(last) - sum(first)
    require(delta >= 0 and type(hz) is int and hz > 0 and 0 < low <= twice / 2 <= high, 'CPU normalization invalid')
    cpu = metric['cpu_counter_normalization']
    for key, value in {'counter_delta_ticks': delta, 'interval_min_ns': low, 'interval_max_ns': high, 'twice_nominal_interval_ns': twice}.items():
        equal(cpu[key], value)
    equal(cpu['nominal_percent_one_core'], delta * 200000000000.0 / (hz * twice))
    equal(cpu['read_window_normalization_percent_range'], [delta * 100000000000.0 / (hz * high), delta * 100000000000.0 / (hz * low)])
    equal(metric['cpu_percent_one_core'], cpu['nominal_percent_one_core'])


def check_namespaces(report, keeper, host):
    names = {'user', 'mnt', 'pid', 'ipc', 'uts', 'net'}
    require(set(report) == set(keeper) == set(host) == names and report == keeper, 'Six namespace records differ')
    for name in names:
        require(all(type(v) is str and re.fullmatch(name + r':\[[0-9]+\]', v) for v in (report[name], host[name])) and report[name] != host[name], 'Namespace not private')


def check_credential(probe):
    require(probe['status'] == 'passed' and probe['architecture'] == 'native x86_64' and
            type(probe['allowed_getpid']) is int and probe['allowed_getpid'] > 0 and probe['actual_x32_or_foreign_abi_execution'] is False, 'Credential probe differs')
    calls = probe['actual_calls']
    require([(v['name'], v['number']) for v in calls] == [('add_key', 248), ('request_key', 249), ('keyctl', 250)], 'Credential syscall inventory differs')
    require(all(v['result'] == -1 and v['errno'] == 1 and v['arguments'] == 'safe invalid, non-mutating' for v in calls), 'Credential denial differs')
    require('NoNewPrivs:\t1' in probe['proc_status'] and 'Seccomp:\t2' in probe['proc_status'], 'Credential filter state missing')


def check_cleanup(receipt, report, keeper, ss):
    require(receipt['status'] == report['status'] == keeper['status'] == ss['status'] == 'passed', 'Original failed status retained')
    require(receipt['exit_code'] == keeper['exit_code'] == ss['command_exit_code'] == 0 and 'forced_outer_stop' not in receipt and report.get('first_error') is None, 'Original failure or forced stop retained')
    cleanup = report['cleanup']
    equal(cleanup['target'], {'exit_code': 0, 'pidfd_exited': True, 'forced': False})
    require(cleanup['webdriver']['exit_code'] == -15 and cleanup['webdriver']['pidfd_exited'] is True and cleanup['webdriver']['forced'] is False, 'Original driver cleanup differs')
    require(cleanup['desktop_pidfd_exit'] is True and cleanup['wm_exit_code'] == 0 and cleanup['errors'] == [] and keeper['forced_cleanup'] == [] and keeper['remaining'] == [], 'Original private cleanup incomplete')
    require(len(ss['processes']) == 2 and all(v['exit_code'] == 0 and v['stopped'] is True for v in ss['processes']) and ss['helper_unchanged'] is True, 'Private Secret Service cleanup differs')
    require(report['paste_prerequisite']['exit_code'] == 0 and report['paste_prerequisite']['native_input_generated'] is False, 'Pre-Desktop paste prerequisite differs')
    return cleanup


def check_host(windows, linux, name, cleanup, keeper_ref, ss_ref):
    for stage, item in windows.items():
        require(set(item) == {'schema_version', 'kind', 'source_revision', 'arm', 'stage', 'collection_status', 'known_compiler_names', 'compiler_processes', 'native_names', 'host_idle', 'observation_performed', 'quiet_window_certified'}, 'Windows null schema differs')
        require(type(item['schema_version']) is int and item['schema_version'] == 1 and item['kind'] == 'phase6_windows_host_unverified' and item['source_revision'] == SOURCE and item['arm'] == name and item['stage'] == stage and item['collection_status'] == 'unverified_no_local_WinAPI', 'Windows context differs')
        require(item['observation_performed'] is False and item['quiet_window_certified'] is False and all(item[k] is None for k in ('known_compiler_names', 'compiler_processes', 'native_names', 'host_idle')), 'Windows unobserved must remain null')
    require(set(windows) == {'preflight', 'postflight'}, 'Both Windows unverified metadata required')
    require(set(linux) == {'schema_version', 'kind', 'source_revision', 'arm', 'host_observation', 'private_cleanup_binding'} and type(linux['schema_version']) is int and linux['schema_version'] == 1 and linux['kind'] == 'phase6_linux_postflight_scoped_observation' and linux['source_revision'] == SOURCE and linux['arm'] == name, 'Linux postflight schema/context differs')
    host = linux['host_observation']
    require(set(host) == {'collection_status', 'compiler_processes', 'known_product_processes', 'observed_display_services', 'host_idle'} and host['collection_status'] == 'observed_linux_proc_names_only' and host['compiler_processes'] == [] and host['known_product_processes'] == [] and host['host_idle'] is None, 'Linux scoped host observation differs')
    display = host['observed_display_services']
    require(type(display) is list and len(display) <= 64, 'Display record bound')
    seen = set()
    for item in display:
        require(set(item) == {'pid', 'name', 'source_status', 'operated'} and type(item['pid']) is int and 0 < item['pid'] < 2**31 and item['name'] in ('Xvfb', 'marco') and item['source_status'] == 'unverified' and item['operated'] is False and (item['pid'], item['name']) not in seen, 'Display attribution differs')
        seen.add((item['pid'], item['name']))
    equal(linux['private_cleanup_binding'], {'cleanup': cleanup, 'keeper': keeper_ref, 'secret_service': ss_ref})


def check_qualification_states(item, wire, renderer, first):
    expected_session = None if item['tag'] == 'first-hotkey' else first['session_id']
    count = 2 if renderer == 'tauri' else 1
    require(len(wire) == count * len(item['observations']), 'Original status/wire correlation count differs')
    for index, observed in enumerate(item['observations']):
        state = observed['status']
        require(type(state) is dict and set(state) == {'renderer', 'managed_pid', 'session_id', 'visible', 'last_error'} and
                state['renderer'] == renderer and type(state['visible']) is bool and state['last_error'] is None, 'Original status shape or activation error differs')
        command = wire[index * count]
        require(command['command'] == 'get_quick_paste_surface_status' and command['reply'] == state, 'Original status reply does not match its observation')
        if renderer == 'mygo':
            pid, session = state['managed_pid'], state['session_id']
            require(pid is None or type(pid) is int and pid == first['pid'], 'Managed PID changed')
            require(pid is not None or session is None, 'Unowned helper session')
            if pid is not None:
                require(type(session) is str and re.fullmatch('[0-9a-f]{32}', session), 'Original MyGo session malformed')
                if expected_session is None:
                    expected_session = session
            require(expected_session is None or session == expected_session, 'Original MyGo session changed')
            require(not state['visible'] or pid == first['pid'], 'Visible helper has no original managed PID')
            accepted = state['visible'] and pid == first['pid'] and expected_session is not None
        else:
            visible = wire[index * count + 1]
            require(state['managed_pid'] is None and state['session_id'] is None and visible['command'] == 'is_quick_paste_visible' and
                    type(visible['reply']) is bool and visible['reply'] is observed['tauri_visible'], 'Original Tauri visibility response differs')
            accepted = visible['reply']
        require(accepted is (index == len(item['observations']) - 1), 'Qualification did not stop at its original first accepted status')
    require(item['final_status'] == item['observations'][-1]['status'] and
            (renderer == 'tauri' or expected_session == first['session_id']), 'Final status/session differs from original replies')


def check_native(report, native, replies, config, resource_rows):
    main, first, renderer = report['original_main'], report['first_hotkey'], config['renderer']
    require((first['pid'] == main['pid']) == (renderer == 'tauri') and all(type(main[k]) is int and main[k] > 0 for k in ('pid', 'started', 'xid')), 'Main/panel identity differs')
    require((renderer == 'tauri' and first['session_id'] is None) or (renderer == 'mygo' and type(first['session_id']) is str and first['session_id']), 'Original renderer session differs')
    require(len(report['warmup']) == 5 and len(report['samples']) == 30, 'Warm/formal counts differ')
    require(all(v['x_errors'] == 0 for v in native), 'Native X error retained')
    native_index = {v['monotonic_ns']: v for v in native}
    require(len(native_index) == len(native), 'Duplicate native timestamps')
    context = report['first_hotkey_context']
    require(context['application_already_started'] is True and context['not_application_cold_start'] is True, 'Activation is not application cold start')
    for show in [first, *report['warmup'], *report['samples']]:
        require(all(show[k] == first[k] for k in ('pid', 'started', 'xid', 'session_id')), 'Original warm process/window/session changed')
        require(show['input']['keys'] == ['Control_L', 'Alt_L', 'Shift_L', 'F10'] and show['native']['active'] == first['xid'] and first['xid'] in show['native']['focus_chain'], 'Real hotkey/native focus differs')
        require(all(type(v) is int for v in (show['input']['input_start_ns'], show['input']['input_synced_ns'], show['native']['monotonic_ns'], show['native_observed_ns'])) and
                0 < show['input']['input_start_ns'] <= show['input']['input_synced_ns'] <= show['native']['monotonic_ns'] <= show['native_observed_ns'], 'Native activation input/observation causality differs')
        original_native = native_index[show['native']['monotonic_ns']]
        require({k: v for k, v in original_native.items() if k != 'tag'} == show['native'] and show['native']['monotonic_ns'] <= show['native_observed_ns'], 'Timed show must bind original raw native observation')
        equal(show['activation_proxy_ms'], (show['native_observed_ns'] - show['input']['input_start_ns']) / 1000000.0)
    equal(report['warm_activation_proxy_ms'], summary([v['activation_proxy_ms'] for v in report['samples']]))
    require(report['retained_ids'] == list(range(1, 11)) and report['capture_owner']['mode'] == 'embedded' and report['capture_owner']['capture_owner'] is True, 'History/capture owner differs')
    require(report['actual_paste']['text'] == config['dataset'][7] and report['actual_paste']['pending'] is False, 'Exact complete row7 paste missing')
    target = report['actual_paste']['windows']['target']
    qualifications = report['show_qualifications']
    tags = ['first-hotkey', *[f'warmup-{i}' for i in range(1, 6)], *[f'samples-{i}' for i in range(1, 31)], 'exact-paste-verification', 'resource-visible']
    require([v['tag'] for v in qualifications] == tags, 'Exact 38 qualification order differs')
    counts = collections.Counter(v['tag'] for v in replies)
    by_tag = {v['tag']: v for v in qualifications}
    timed = dict(zip(tags[:36], [first, *report['warmup'], *report['samples']]))
    for item in qualifications:
        endpoint = item['original_endpoint']
        require(item['qualified'] is True and item['native_proxy_endpoint_unchanged'] is True and all(endpoint[k] == first[k] for k in ('pid', 'started', 'xid')), 'Qualification original endpoint differs')
        require(item['native_proxy_endpoint_ns'] == endpoint['native_observed_ns'] <= item['qualification_completed_ns'] and item['qualification_completed_ns'] / 1e9 < item['original_deadline_monotonic'], 'Qualification deadline differs')
        require(item['reply_records'] == counts[item['tag']] == item['status_reads'] * (2 if renderer == 'tauri' else 1) and item['status_reads'] == len(item['observations']) >= 1, 'Qualification reply counts differ')
        if item['tag'] in timed:
            require(timed[item['tag']]['native_observed_ns'] == endpoint['native_observed_ns'], 'Timed native endpoint moved')
        final = item['final_status']
        require((renderer == 'mygo' and final['managed_pid'] == first['pid'] and final['session_id'] == first['session_id'] and final['visible'] is True) or (renderer == 'tauri' and final['renderer'] == 'tauri' and final['managed_pid'] is None and final['session_id'] is None), 'Qualified renderer status differs')
        for observation in item['observations']:
            if renderer == 'tauri':
                require(observation['tauri_visible'] is True and observation['status'] == final, 'Tauri visibility qualification differs')
            for side in ('before', 'after'):
                current = native_index[observation[side + '_native_ns']]
                require(current['active'] == first['xid'] and first['xid'] in current['focus_chain'], 'Qualification focus changed')
                windows = [v for v in current['windows'] if v['xid'] == first['xid']]
                require(len(windows) == 1 and windows[0]['pids'] == [first['pid']] and windows[0]['viewable'] is True, 'Qualification window changed')
                for role, original in (('main', main), ('panel', first)):
                    require(all(observation[side + '_identities'][role][k] == original[k] for k in ('pid', 'started')), 'Qualification process generation changed')
        check_qualification_states(item, [r for r in replies if r['tag'] == item['tag']], renderer, first)
    for reply in replies:
        item = by_tag[reply['tag']]
        require('error' not in reply and reply['http_status'] == 200 and reply['original_endpoint'] == item['original_endpoint'], 'Original qualification reply failed')
        require(item['native_proxy_endpoint_ns'] <= reply['request_started_ns'] <= reply['response_finished_ns'] <= item['qualification_completed_ns'] and reply['response_finished_ns'] / 1e9 < item['original_deadline_monotonic'], 'Reply outside original deadline')
        require(0 < reply['remaining_budget_ms'] <= 5000 and 0 <= reply['wire_bytes'] <= 65536 and abs(item['original_deadline_monotonic'] - (reply['request_started_ns'] / 1e9 + reply['remaining_budget_ms'] / 1000)) < .02, 'Reply quota/budget differs')
        require(reply['actual_response']['value']['ok'] is True and reply['actual_response']['value']['value'] == reply['reply'], 'Original wire response differs')
        require(reply['command'] in ('get_quick_paste_surface_status', 'is_quick_paste_visible'), 'Unexpected qualification command')
        if reply['command'] == 'is_quick_paste_visible':
            require(renderer == 'tauri' and reply['reply'] is True, 'Tauri visibility reply differs')
        elif renderer == 'tauri':
            require(reply['reply'] == item['final_status'], 'Tauri status reply differs')
    require(len(replies) == sum(v['reply_records'] for v in qualifications), 'Unqualified extra replies')
    for state, visible in (('visible', True), ('hidden', False)):
        samples = resource_rows[state]
        observations = [v for v in native if v['tag'] == 'resource-' + state and
                        samples[0]['monotonic_ns'] <= v['monotonic_ns'] <= samples[-1]['cpu_counter_read_window_ns'][1]]
        require(len(observations) == len(samples) == 960, 'Resource native/row count differs')
        for sample, observed in zip(samples, observations):
            require(sample['monotonic_ns'] <= observed['monotonic_ns'] <= sample['cpu_counter_read_window_ns'][0], 'Native observation outside capture')
            for original, shown in ((main, False), (first, visible)):
                windows = [w for w in observed['windows'] if w['xid'] == original['xid']]
                require(len(windows) == 1 and windows[0]['pids'] == [original['pid']] and windows[0]['viewable'] is shown, 'Resource original window state differs')
            window = next(w for w in observed['windows'] if w['xid'] == first['xid'])
            require(window['rect']['width'] == 480 and window['rect']['height'] == 360, 'Original panel geometry differs')
            focused = first['xid'] if visible else target
            require(observed['active'] == focused and focused in observed['focus_chain'], 'Resource foreground differs')


def digest_ref(store, reference, expected, role=None):
    require(reference['sha256'] == expected, 'Frozen original digest differs')
    return store.item(reference, role)


def check_sources(store, full):
    public = store.manifest
    for key, digest in [('summary', SUMMARY_SHA), ('integration_manifest', MANIFEST_SHA),
                        ('reader', READER_SHA), ('reader_freeze', READER_FREEZE_SHA),
                        ('root_final_review', ROOT_FINAL_SHA), ('independent_summary_audit', AUDIT_SHA),
                        ('independent_summary_source', AUDIT_SOURCE_SHA)]:
        digest_ref(store, public[key], digest)
    equal(full['integration_manifest'], public['integration_manifest'])
    equal(full['root_final_review'], public['root_final_review'])
    manifest = store.json(public['integration_manifest'])
    method(manifest)
    require(manifest['source_revision'] == SOURCE and manifest['kind'] == 'phase6_F2_process_only_integration_manifest', 'Integration manifest identity differs')
    require(set(manifest['sources']) == {'method', 'foundation', 'native', 'integration'}, 'Four source groups required')
    require({key: len(value) for key, value in manifest['sources'].items()} ==
            {'method': 9, 'foundation': 7, 'native': 4, 'integration': 6}, 'Reviewed source inventory differs')
    for group in manifest['sources'].values():
        for reference in group.values():
            store.item(reference, 'source')
    policy = store.json(manifest['policy'])
    for group, pins in [('method', policy['method_pins']), ('foundation', policy['foundation_pins']), ('native', policy['native_pins'])]:
        require(set(manifest['sources'][group]) == set(pins), 'Source role inventory differs')
        for name, pin in pins.items():
            equal({key: manifest['sources'][group][name][key] for key in ('bytes', 'sha256')}, pin)
    for name, pin_name in [('assess_descriptive.py', 'assess_descriptive.py'), ('test_process_only.py', 'test_process_only.py'),
                           ('process-only-method.plan.json', 'source-plan.json')]:
        original = str(PurePosixPath(manifest['plan']['path']).parent / name)
        item = store.item(original, 'source')
        equal({key: item[key] for key in ('bytes', 'sha256')}, policy['native_pins'][pin_name])
    plan, preparation = store.json(manifest['plan']), store.json(manifest['preparation'])
    method(plan)
    method(preparation)
    require(plan['root_go'] is False and plan['performance_eligible'] is False and plan['arms'] == list(ARMS) and
            plan['expected']['source_revision'] == SOURCE and preparation['status'] == 'prepared_only' and
            preparation['native_executed'] is False and preparation['root_go'] is False and
            preparation['baseline_tests_executed_as_v2'] is False, 'Prepared-only method scope differs')
    equal(preparation['sources_after'], manifest['sources'])
    equal(preparation['plan'], manifest['plan'])
    equal(full['inputs'], plan['inputs'])
    store.item(full['summarizer'], 'source')
    for reference in manifest['pending'].values():
        store.item(reference, 'cohort')
    for reference in manifest['prerequisites'].values():
        store.item(reference, 'proof')
    baseline_path = str(PurePosixPath(manifest['policy']['path']).parent.parent /
                        'phase6-linux-count-prepare-1fe685c-20261102/source-plan.json')
    digest_ref(store, store.ref(baseline_path), 'cc3922225451d5c1ae742e10ce8b364890b78104a2a3266eb487bc7ff0915c82', 'source')
    baseline = store.json(baseline_path)
    equal(plan['expected'], baseline['expected'])
    for key, expected in baseline['fixed_prerequisites'].items():
        equal(manifest['prerequisites'][key], expected)
    four = {k: plan['inputs'][k] for k in ('desktop', 'helper', 'desktop_proof', 'helper_proof')}
    for name, reference in manifest['prerequisites'].items():
        value = store.json(reference)
        if name == 'input_provenance':
            equal(value['expected'], plan['expected'])
            equal(value['inputs'], four)
            continue
        stages = 13 if name.startswith('idle_') else 19
        require(value['source_revision'] == SOURCE and value['status'] == 'passed' and
                type(value['actual_stages']) is int and value['actual_stages'] == stages, 'Frozen prerequisite metadata differs')
        key = 'runtime_input_pair' if name == 'idle_R3_assessment' else 'input_pair_and_dependencies' if name == 'original19_assessment' else 'original_inputs'
        equal({k: value[key][k] for k in four}, four)
        if stages == 19:
            equal(value['phase_names'], baseline['actual19_phase_names'])
        else:
            phases = [v['name'] for v in value['phases']] if name.endswith('_assessment') else value['phase_names']
            equal(phases, ['real-ewmh-wm', 'actual-native-surface-identity', 'actual-desktop-identity',
                'actual-private-data-path', 'real-host-seed-ten', 'first-native-show', 'actual-initial-hide',
                'idle-before-real-registered-hotkey', 'same-original-helper-after-idle-hotkey',
                'idle-recovery-query-and-one-enter', 'idle-before-normal-disable',
                'normal-disable-confirms-original-helper-exit', 'owner-history-retained'], 'Actual R3 metadata order')
    freeze = store.json(public['reader_freeze'])
    require(freeze['kind'] == 'phase6_process_only_v2_Root_independent_reader_source_freeze' and
            freeze['status'] == 'source_reviewed_frozen_only' and freeze['source_revision'] == SOURCE and
            freeze['actual_reader_executed'] is False and freeze['native_executed'] is False and freeze['root_go'] is False,
            'A source freeze is not a native or raw-review result')
    method(freeze)
    scope(freeze)
    require(set(freeze['sources']) == {'review_arm.py', 'test_review_arm.py', 'source-plan.json'}, 'Reader source set differs')
    for name, pin in freeze['sources'].items():
        store.item({'path': freeze['stage'].replace('\\', '/').rstrip('/') + '/' + name, **pin}, 'source')
    store.item({k: freeze['independent_source_review'][k] for k in ('path', 'bytes', 'sha256')}, 'proof')
    require(set(public['public_sources']) == {'assemble.py', 'verify.py', 'test_bundle.py', 'source-plan.json'}, 'Four public-bundle sources required')
    for reference in public['public_sources'].values():
        store.item(reference, 'source')
    require(store.data(public['public_sources']['verify.py']) == Path(__file__).read_bytes(),
            'Use the exact archived portable verifier source; no ZIP code is executed')
    root_final = store.json(public['root_final_review'])
    require(root_final['kind'] == 'phase6_F2_process_only_summary_review' and root_final['source_revision'] == SOURCE and
            root_final['performance_eligible'] is False and root_final['phase6_passed'] is False, 'Root final review scope differs')
    method(root_final)
    equal(root_final['manifest'], public['integration_manifest'])
    require(type(full['arms_in_execution_order']) is list and len(full['arms_in_execution_order']) == len(root_final['arms']) == 4, 'Four final arm bindings required')
    for arm, binding in zip(full['arms_in_execution_order'], root_final['arms']):
        equal({key: arm[key] for key in ('assessment', 'independent_review', 'root_png', 'copy_proof')}, binding)
    return manifest, plan


def check_omitted(store, inputs):
    values = store.manifest['omitted_elf_inputs']
    require(type(values) is list and len(values) == 7 and {v['name'] for v in values} == OMITTED, 'Exactly seven omitted ELF roles required')
    require(len(inputs) == 11, 'Eleven recorded inputs required')
    for value in values:
        require(set(value) == {'name', 'record', 'elf_identity'}, 'Exact omitted identity schema required')
        equal(value['record'], inputs[value['name']], 'Omitted historical bytes/SHA identity only')
        require(value['record']['path'] not in store.originals, 'Omitted bytes must not be silently included')
        header = value['elf_identity']['header_hex']
        require(type(header) is str and re.fullmatch('[0-9a-f]{128}', header), 'Exact recorded64 header required')
        equal(value['elf_identity'], elf_header(bytes.fromhex(header)))
    for name, reference in inputs.items():
        if name not in OMITTED:
            store.item(reference)


def check_private_records(store, raw, report, keeper, ss, config):
    require(ss['backend'] == 'explicit_secret_service' and ss['keyutils_constructed'] is False and
            ss['system_keyring_isolation_claimed'] is False and ss['helper'] == '/fixture-bin/private-ss-preseed' and
            ss['helper_sha256'] == ss['helper_sha256_after'] == config['inputs']['private_ss_helper']['sha256'], 'Private SS helper/backend differs')
    require(type(ss['private_root']) is str and re.fullmatch('/q/dezirclip-private-ss-[A-Za-z0-9_-]+', ss['private_root']), 'Private SS root differs')
    require([(v['operation'], v['exit_code']) for v in ss['steps']] == [('seed', 0), ('check', 0), ('seed', 1)] and
            parse(ss['steps'][2]['stderr'])['reason'] == 'ExistingCredentialRefused', 'Private SS positive/negative proof differs')
    require(ss['command'][-2:] == ['/fixture-bin/inner.py', '--inside'] and keeper['command'][:6] ==
            ['/usr/bin/xvfb-run', '-f', '/q/phase6.xauthority', '-a', '-s', '-screen 0 1280x900x24 -noreset'] and
            keeper['command'][-len(ss['command']):] == ss['command'], 'Private X11/inner command differs')
    probe = report['paste_prerequisite']
    require(probe['tool'] == config['inputs']['paste_tool'] and probe['proof'] == config['inputs']['paste_tool_proof'] and
            probe['executable_run'] is True and probe['command'] == ['/fixture-bin/xdotool', 'version'] and
            probe['started_ns'] <= probe['finished_ns'] and probe['private_namespaces'] == report['private_namespaces'], 'Original paste dependency proof differs')
    for stream in ('stdout', 'stderr'):
        reference = store.ref(str(raw / ('functional/paste-tool-version.' + stream + '.log')))
        equal(probe[stream], {k: reference[k] for k in ('bytes', 'sha256')})
        require(reference['bytes'] <= 4096, 'Paste dependency stream bound')
    require(b'3.20160805.1' in store.data(str(raw / 'functional/paste-tool-version.stdout.log'), 4096), 'Paste dependency version absent')


def check_png(store, name, raw, report, config, binding):
    setup = report['resource_query_setup']
    require(setup['query'] == 'dzcp6needle7' and setup['expected_content'] == config['dataset'][7] and
            setup['stored_rows'] == 10 and setup['expected_matching_rows'] == 1 and setup['setup_wait_seconds'] == .7,
            'Original query workload differs')
    proof = store.json(binding['copy_proof'])
    source = store.ref(str(raw / 'functional/resource-visible-query.png'))
    copied = store.ref(store.at('cohort/' + name + '.setup.png'))
    require(not (set(proof) & {'postflight_linux_owned_native', 'whole_host_idle', 'quiet_window_certified'}), 'Legacy host claim rejected')
    equal(proof['source'], source)
    equal(proof['copy'], copied)
    require(proof['sha256_verified_equal'] is True and source['sha256'] == copied['sha256'] == setup['screenshot']['sha256'], 'Original setup PNG hashes differ')
    pixels = store.data(source, 8 * 1024 * 1024)
    require(len(pixels) >= 24 and pixels[:8] == b'\x89PNG\r\n\x1a\n' and struct.unpack('>II', pixels[16:24]) == (480, 360), 'Original PNG geometry differs')
    view = store.json(binding['root_png'])
    require(view['actual_view_completed'] is True and view['reviewer'] == 'Root' and view['status'] == 'passed' and
            view['source_revision'] == SOURCE and view['arm'] == name and view['png_sha256'] == source['sha256'] and
            view['exact_query'] == setup['query'] and view['visible_matching_rows'] == 1 and
            view['full_visible_content'] == config['dataset'][7], 'Saved actual Root PNG-view receipt differs')
    return proof


def check_arm(store, arm, binding, manifest, plan, earlier):
    name = arm['run_name']
    raw = PurePosixPath(RAW_ORIGINAL) / name
    def cohort(suffix):
        return store.at('cohort/' + name + '.' + suffix)
    for key, suffix in [('assessment', 'assessment.json'), ('independent_review', 'independent-raw-review.json'),
                        ('root_png', 'root-png.json'), ('copy_proof', 'setup-copy-proof.json')]:
        equal(binding[key], store.ref(cohort(suffix)))
    pending, config = store.json(cohort('pending.json')), store.json(cohort('execution.json'))
    equal(pending, {**plan, **arm})
    require(pending['root_go'] is False and config == {**pending, 'root_go': True}, 'Only original once-GO may change')
    method(config)
    assessment, review = store.json(binding['assessment']), store.json(binding['independent_review'])
    for value in (assessment, review):
        method(value)
        scope(value)
        require(value['source_revision'] == SOURCE, 'Arm assessment/review source differs')
    require(assessment['kind'] == 'phase6_count_paced_process_only_arm_assessment' and
            assessment['status'] == 'passed_limited_process_only_consistency' and assessment['arm'] == arm,
            'Original limited Root assessment differs')
    require(set(review) == FIELDS and review['kind'] == 'phase6_F2_process_only_independent_raw_review' and
            review['status'] == 'passed_limited_independent_process_only_raw_consistency' and review['arm'] == name and
            review['renderer'] == arm['renderer'] and type(review['arm_index']) is int and review['arm_index'] == arm['arm_index'] and
            review['reviewer_role'] == 'independent_raw_reviewer' and review['independent_from_authorizer_and_assessor_author'] is True and
            review['actual_original_raw_review_completed'] is True and review['whole_ABBA_complete'] is False and
            set(review['checks']) == CHECKS and all(v is True for v in review['checks'].values()), 'Original closed independent review differs')
    digest_ref(store, binding['independent_review'], REVIEWS[arm['arm_index'] - 1], 'cohort')
    equal(review['root_assessment'], binding['assessment'])
    equal(review['root_png_evidence'], binding['root_png'])
    equal(review['setup_copy_proof'], binding['copy_proof'])
    equal(review['integration_manifest'], store.manifest['integration_manifest'])
    equal(review['reviewer_reader'], store.manifest['reader'])
    equal(review['execution'], store.ref(cohort('execution.json')))
    equal(assessment['execution'], review['execution'])
    store.item(review['assessor'], 'source')
    equal(assessment['assessor'], review['assessor'])
    originals = assessment['raw_evidence']
    require(type(originals) is list and len(originals) == 34 and
            {v['path'] for v in originals} == {str(raw / n) for n in RAW_NAMES}, 'Exact original34 raw paths required')
    equal(review['raw_evidence'], originals)
    equal(binding['raw_evidence'], originals)
    for reference in originals:
        item = store.item(reference, 'raw')
        require(item['archive_path'] == 'raw/' + name + '/' + str(PurePosixPath(reference['path']).relative_to(raw)), 'Raw path map crosses arms')
    receipt = store.json(str(raw / 'receipt.json'))
    runtime = store.json(str(raw / 'runtime.json'))
    report = store.json(str(raw / 'functional/performance.json'))
    keeper = store.json(str(raw / 'functional/keeper.json'))
    ss = store.json(str(raw / 'functional/private-ss/proof.json'))
    cleanup = check_cleanup(receipt, report, keeper, ss)
    method(runtime)
    method(report)
    for value in (receipt, report):
        require(value['source_revision'] == SOURCE and type(value['arm_index']) is int and
                value['arm_index'] == arm['arm_index'] and value['renderer'] == arm['renderer'], 'Original raw arm identity differs')
    require(receipt['runtime_sha256'] == store.ref(str(raw / 'runtime.json'))['sha256'], 'Original runtime hash differs')
    for key in ('inputs', 'expected', 'comparison', 'fixture_sources', 'pure_pacer'):
        equal(runtime[key], config[key])
    require(report['performance_eligible'] is False and all(report[k] is None for k in
            ('first_interactive_frame', 'search_ready_latency', 'gpu_memory', 'wakeup_count', 'ime_dpi_multimonitor')),
            'Unmeasured product metrics must remain null')
    check_namespaces(report['private_namespaces'], keeper['private_namespaces'], runtime['host_namespaces'])
    check_credential(report['credential_boundary'])
    boundary = store.json(str(raw / 'credential-boundary.json'))
    source = manifest['sources']['foundation']
    require(boundary['status'] == 'boundary_installed_and_native_probe_passed' and
            boundary['canonical_module_sha256'] == source['canonical/credential_guard.py']['sha256'] and
            boundary['bpf_sha256'] == source['canonical/credential-deny.bpf']['sha256'] and
            boundary['launcher_sha256'] == source['canonical/credential_launcher.py']['sha256'], 'Credential source proof differs')
    check_credential(boundary['native_probe'])
    require(receipt['command'][1:3] == ['-s', '-B'] and receipt['command'][4] == '--proof' and
            receipt['command'][6] == '--' and boundary['command'] == receipt['command'][7:] and
            boundary['pid'] == boundary['native_probe']['allowed_getpid'], 'Credential launcher command/PID differs')
    check_private_records(store, raw, report, keeper, ss, config)
    for key in ('surface_build_info', 'desktop_build_info'):
        fields = config['expected'] if key == 'surface_build_info' else ('component_version', 'source_revision', 'source_state', 'build_id', 'target')
        require(all(report[key][field] == config['expected'][field] for field in fields), 'Recorded product build identity differs')
    equal(store.json(str(raw / 'functional/surface-build-info.stdout')), report['surface_build_info'])
    require(report['surface_build_info']['mygo_version'] == 'v0.6.4' and
            report['surface_build_info']['mygo_source_revision'] == '6255ea40139d06e0762a7c4a32626148ba410478', 'Recorded MyGo identity differs')
    resource_rows = {}
    for state in ('visible', 'hidden'):
        resource_rows[state] = store.lines(str(raw / ('functional/resources-' + state + '.jsonl')))
        check_resource(resource_rows[state], store.lines(str(raw / ('functional/resource-clock-' + state + '.jsonl'))),
                       report['resource_sampling_progress'][state], report['resources'][state], report['original_main'], report['first_hotkey'])
    replies = store.lines(str(raw / 'functional/show-status-qualification.jsonl'))
    check_native(report, store.lines(str(raw / 'functional/native-observations.jsonl')), replies, config, resource_rows)
    target = store.lines(str(raw / 'functional/target.stdout.jsonl'))
    require(target[0]['ready'] is True and type(target[0]['pid']) is int and target[0]['pid'] > 0 and
            target[0]['windows'] == report['actual_paste']['windows'], 'Original target identity differs')
    target_replies = target[1:]
    require([v['id'] for v in target_replies] == list(range(1, len(target_replies) + 1)) and
            all(v['windows'] == target[0]['windows'] for v in target_replies) and report['actual_paste'] in target_replies and
            all(v['text'] in ('', config['dataset'][7]) for v in target_replies) and
            all(a['monotonic_ns'] <= b['monotonic_ns'] for a, b in zip(target_replies, target_replies[1:])),
            'Original target full text/reply sequence differs')
    require(store.json(str(raw / 'host-preflight.json'))['compiler_processes'] == [] and
            all(v['compiler_processes'] == [] for v in store.lines(str(raw / 'host-load.jsonl'))), 'Recorded known compiler contamination retained')
    proof = check_png(store, name, raw, report, config, binding)
    windows = {stage: store.json(cohort(stage + '-windows.json')) for stage in ('preflight', 'postflight')}
    check_host(windows, proof['postflight_linux_scope'], name, cleanup,
               store.ref(str(raw / 'functional/keeper.json')), store.ref(str(raw / 'functional/private-ss/proof.json')))
    equal(assessment['verified_resources'], report['resources'])
    equal(binding['resources'], report['resources'])
    equal(assessment['warm_activation_proxy_ms'], report['warm_activation_proxy_ms'])
    equal(binding['warm_activation_proxy_ms'], report['warm_activation_proxy_ms'])
    require(assessment['qualification_count'] == 38 and assessment['actual_reply_count'] == len(replies) and
            assessment['cleanup'] == cleanup, 'Root assessment does not match recomputed original records')
    equal(assessment['root_png_evidence'], binding['root_png'])
    equal(assessment['setup_copy_proof'], binding['copy_proof'])
    root = store.json(cohort('root-once-review.json'))
    authorization = store.json(cohort('authorization.json'))
    schedule = store.json(cohort('scheduling.json'))
    preflight = store.json(cohort('preflight-linux.json'))
    for value in (root, authorization):
        method(value)
        require(value['source_revision'] == SOURCE and value['arm'] == arm and value['performance_eligible'] is False, 'Original once-authorization identity differs')
    require(root['kind'] == 'phase6_F2_process_only_root_once_review' and root['root_once_statement'] ==
            f'Authorize {name} once; no retry, replacement or next arm.' and
            authorization['kind'] == 'phase6_F2_process_only_one_arm_authorization' and
            authorization['root_go'] is True and authorization['native_executed_by_authorizer'] is False and
            authorization['next_arm_authorized'] is False, 'Original one-shot boundary differs')
    equal(root['pending'], manifest['pending'][name])
    equal(authorization['pending'], root['pending'])
    equal(root['manifest'], store.manifest['integration_manifest'])
    equal(authorization['manifest'], root['manifest'])
    equal(authorization['root_review'], store.ref(cohort('root-once-review.json')))
    equal(authorization['scheduling'], store.ref(cohort('scheduling.json')))
    equal(root['prior_arms'], earlier)
    equal(authorization['prior_arms'], earlier)
    equal(schedule['windows'], store.ref(cohort('preflight-windows.json')))
    equal(schedule['linux'], store.ref(cohort('preflight-linux.json')))
    require(schedule['whole_host_idle'] is None and schedule['root_scheduling_statement'] == root['root_scheduling_statement'] and
            type(root['root_scheduling_statement']) is str and 1 <= len(root['root_scheduling_statement']) <= 2048,
            'Scheduling declaration is not verified host quiet')
    require(preflight['source_revision'] == SOURCE and preflight['arm'] == name and len(preflight['samples']) == 3, 'Three original metadata samples required')
    for sample in preflight['samples']:
        names = sample['strict_proc_names']
        require(sample['original_host_activity']['compiler_processes'] == [] and
                names['compiler_processes'] == [] and names['known_product_processes'] == [] and names['host_idle'] is None,
                'Original compiler/product preflight failed or claimed idle')
        for display in names['observed_display_services']:
            require(display['name'] in ('Xvfb', 'marco') and display['source_status'] == 'unverified' and display['operated'] is False,
                    'Preflight display services cannot be certified as owned or quiet')
    return report, resource_rows


def aggregate(reports):
    require(len(reports) == 4, 'Complete four-arm comparison required')
    result = {}
    for renderer in ('tauri', 'mygo'):
        selected = [(report, rows) for report, rows in reports if report['renderer'] == renderer]
        require(len(selected) == 2, 'Two original arms per renderer required')
        latencies = [v['activation_proxy_ms'] for report, unused in selected for v in report['samples']]
        require(len(latencies) == 60, 'Sixty formal samples per renderer required')
        result[renderer] = {'warm_activation_proxy_ms': summary(latencies), 'resources': {}}
        for state in ('visible', 'hidden'):
            metrics = [report['resources'][state] for report, unused in selected]
            require(all(v['stable_process_membership'] is True for v in metrics), 'Pooled CPU requires stable original process families')
            clocks = [v['cpu_counter_normalization'] for v in metrics]
            hz = {v['cpu_clock_ticks_per_second'] for v in metrics}
            require(len(hz) == 1, 'CPU tick frequency differs')
            hz = hz.pop()
            ticks = sum(v['counter_delta_ticks'] for v in clocks)
            twice = sum(v['twice_nominal_interval_ns'] for v in clocks)
            low, high = sum(v['interval_min_ns'] for v in clocks), sum(v['interval_max_ns'] for v in clocks)
            require(type(hz) is int and hz > 0 and ticks >= 0 and 0 < low <= twice / 2 <= high, 'Pooled CPU read-window intervals differ')
            cpu = {'counter_delta_ticks': ticks, 'ticks_per_second': hz, 'sum_twice_nominal_intervals_ns': twice,
                   'nominal_percent_one_core': ticks * 2e11 / (hz * twice),
                   'read_window_normalization_percent_range': [ticks * 1e11 / (hz * high), ticks * 1e11 / (hz * low)],
                   'scope': 'Duration-weighted disjoint first/last counter-read intervals. Not elapsed-window/tail CPU, a common-time family bound, tick-quantization bound or statistical confidence interval.'}
            values = [row for unused, rows in selected for row in rows[state]]
            require(len(values) == 1920, 'All pooled1920 snapshots required')
            result[renderer]['resources'][state] = {'empirical_snapshot_pss_kib': summary([v['pss_kib'] for v in values]),
                'empirical_snapshot_uss_kib': summary([v['uss_kib'] for v in values]), 'cpu_counter_interval': cpu,
                'stable_thread_membership': None, 'observed_thread_context_switch_delta': None, 'wakeup_count': None,
                'thread_context_switch_collection': COLLECTION}
    return result


def verify(path):
    with zipfile.ZipFile(path) as archive:
        store = Bundle(archive)
        full = store.json(store.manifest['summary'])
        require(full['kind'] == 'complete_F2_process_only_v2_limited_comparison' and
                full['status'] == 'completed_limited_process_only_consistency' and full['source_revision'] == SOURCE,
                'Frozen summary identity differs')
        method(full)
        scope(full)
        require(full['context_switches'] is None and full['wakeup_count'] is None, 'Not-collected counters cannot become zero')
        manifest, plan = check_sources(store, full)
        check_omitted(store, full['inputs'])
        reports, earlier = [], []
        for arm, binding in zip(ARMS, full['arms_in_execution_order']):
            require(binding['arm'] == arm['run_name'] and binding['renderer'] == arm['renderer'], 'ABBA order differs')
            reports.append(check_arm(store, arm, binding, manifest, plan, earlier))
            earlier.append({key: binding[key] for key in ('assessment', 'independent_review', 'root_png', 'copy_proof')})
        computed = aggregate(reports)
        equal(full['variants'], computed, 'frozen aggregate')
        audit = store.json(store.manifest['independent_summary_audit'])
        method(audit)
        scope(audit)
        require(audit['kind'] == 'phase6_F2_process_only_v2_independent_summary_audit' and
                audit['status'] == 'passed_limited_independent_summary_consistency' and audit['source_revision'] == SOURCE and
                audit['formal_activations_total'] == 120 and audit['resource_rows_total'] == audit['raw_clock_rows_total'] == 7680 and
                audit['closed_raw_originals_total'] == 136 and audit['actual_native_or_product_executed'] is False and
                audit['author_summarizer_executed'] is False and audit['original_reader_rerun'] is False,
                'Saved independent file-only audit scope differs')
        equal(audit['summary'], store.manifest['summary'])
        equal(audit['integration_manifest'], store.manifest['integration_manifest'])
        equal(audit['root_final_review'], store.manifest['root_final_review'])
        equal(audit['independent_audit_reader'], store.manifest['independent_summary_source'])
        equal(audit['variants_recomputed_from_original_raw'], computed, 'independent aggregate')
        require(store.used == set(store.members), 'An unreferenced extra file is not part of this closed evidence bundle')
        return {'status': 'offline_process_only_consistency_and_included_hashes_match', 'source_revision': SOURCE,
                'observer_method': METHOD, 'thread_context_switch_collection': COLLECTION, 'included_files': len(store.members),
                'closed_raw_files': 136, 'cohort_control_files': 58, 'formal_latencies': 120,
                'resource_captures': 7680, 'resource_clock_rows': 7680, 'omitted_elf_inputs': 7,
                'omitted_full_hashes_rechecked': False, 'prerequisite_check_scope': 'Saved metadata and included proof bytes only; not a new native prerequisite run.',
                'variants': computed, 'performance_eligible': False, 'phase6_passed': False, 'phase7_authorized': False,
                'next_arm_authorized': False, 'whole_host_idle': None, 'quiet_window': None, 'performance_acceptance': None,
                'limits': ['Reads and recomputes archived records only; never reruns native input, isolation, visual acceptance or a product.',
                           'Saved Root PNG-view receipts and private-cleanup records are checked, not reenacted.',
                           'Seven omitted ELF inputs have only recorded SHA/size/header identity; their complete bytes were not rehashed by this portable verifier.',
                           'Windows/whole-host quiet, first interactive frame, GPU, wakeups, IME/DPI/multimonitor and Phase6/Phase7 acceptance remain unknown or false.']}


if __name__ == '__main__':
    require(len(sys.argv) == 2, 'Usage: python -s -B verify.py bundle.zip')
    print(json.dumps(verify(sys.argv[1]), indent=2, allow_nan=False))
