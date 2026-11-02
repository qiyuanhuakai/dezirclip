"""Recompute frozen count-v1 statistics from a ZIP; never extract or execute it."""
import hashlib
import json
import math
from pathlib import PurePosixPath
import re
import stat
import sys
import zipfile

REVISION = '5d6af007b0b000ed78a22e7fc6e1413c92f28d42'
COMPARISON_SHA = '241fa784d832bf67be7406a553fa7e8397c934d1897f9d4a3098276030221c35'
FREEZE_SHA = '63303598ff00a400651c1f8d3907c43917bec7739201445dc22baa4df7706a5b'


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
        require(type(actual) in (int, float) and math.isfinite(actual)
                and math.isclose(actual, expected, rel_tol=1e-12, abs_tol=1e-9), path + ': number differs')
    else:
        require(actual == expected, path + ': value differs')


def safe_name(name):
    path = PurePosixPath(name)
    return (bool(re.fullmatch(r'[A-Za-z0-9._/-]+', name)) and not path.is_absolute()
            and '..' not in path.parts and str(path) == name and not name.endswith('/'))


def read_archive(archive):
    entries = archive.infolist()
    names = [item.filename for item in entries]
    require(len(names) == len(set(names)) and all(safe_name(name) for name in names), 'Unsafe/duplicate ZIP name')
    require(all(not stat.S_ISLNK(item.external_attr >> 16) for item in entries), 'ZIP symlink rejected')
    require(len(entries) <= 180 and sum(item.file_size for item in entries) <= 220_000_000,
            'Unexpected archive inventory size')
    require(all(item.file_size <= 32_000_000 for item in entries), 'Unexpected member size')
    require('manifest.json' in names and archive.getinfo('manifest.json').file_size <= 1_000_000, 'Manifest missing/oversized')
    manifest = json.loads(archive.read('manifest.json'))
    require(manifest['kind'] == 'phase6_linux_count_v1_public_analysis_inputs'
            and manifest['source_revision'] == REVISION, 'Unexpected bundle identity')
    files = manifest['files']
    require(len(files) == 164 and len({item['original_path'] for item in files}) == len(files)
            and len({item['archive_path'] for item in files}) == len(files), 'Inventory count/path mismatch')
    require(set(names) == {'manifest.json'} | {item['archive_path'] for item in files}, 'Unexpected or missing ZIP member')
    require(sum(item['role'] == 'raw' for item in files) == 136, 'Original raw inventory differs')
    require(len(manifest['omitted_elf_inputs']) == 7, 'Omitted ELF identity list differs')
    by_original = {}
    for item in files:
        require(safe_name(item['archive_path']), 'Unsafe manifest member')
        data = archive.read(item['archive_path'])
        require(len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256'],
                'Member bytes/SHA differ: ' + item['archive_path'])
        require(data[:4] != b'\x7fELF', 'Executable input must not be included')
        by_original[item['original_path']] = item
    return manifest, by_original


def check_resource(rows, clocks, progress, metric, main, panel):
    require(len(rows) == len(clocks) == len(progress['clocks']) == 960, 'Resource/clock count differs')
    require(progress['requested_count'] == progress['capture_calls'] == progress['samples_written'] == 960,
            'Resource capture accounting differs')
    require(progress['kind'] == 'event-anchored-count-v1' and progress['status'] == 'passed'
            and metric['sampling_protocol'] == 'event-anchored-count-v1' and metric['sample_count'] == 960,
            'Resource protocol differs')
    require(progress['interval_ns'] == 250_000_000 and 0 < progress['deadline_ns'] - progress['started_ns'] <= 1_260_000_000_000,
            'Resource interval/deadline differs')
    families, threads = [], []
    previous = None
    for index, (row, clock, saved) in enumerate(zip(rows, clocks, progress['clocks']), 1):
        equal(clock, {key: value for key, value in saved.items() if key != 'clock_log_end_ns'}, 'raw clock')
        equal(row['sampling_clock'], {key: saved[key] for key in ('index', 'due_ns', 'actual_start_ns', 'late_ns', 'capture_end_ns')})
        require(saved['index'] == row['snapshot_index'] == index and row['monotonic_ns'] == saved['actual_start_ns'], 'Sample index/time differs')
        require(saved['due_ns'] == (previous['actual_start_ns'] + 250_000_000 if previous else progress['started_ns']), 'Pacing differs')
        require(saved['late_ns'] == saved['actual_start_ns'] - saved['due_ns'] >= 0, 'Lateness differs')
        require(saved['actual_start_ns'] <= saved['capture_end_ns'] <= saved['serialization_end_ns']
                <= saved['sample_write_end_ns'] <= saved['clock_log_end_ns'] < progress['deadline_ns'], 'Clock ordering differs')
        require(0 <= row['observer_cost_ns'] <= saved['capture_end_ns'] - saved['actual_start_ns'], 'Observer cost differs')
        bounds = row['cpu_counter_read_window_ns']
        require(len(bounds) == 2 and saved['actual_start_ns'] <= bounds[0] <= bounds[1] <= saved['capture_end_ns'], 'CPU read window differs')
        if previous:
            require(saved['actual_start_ns'] >= previous['clock_log_end_ns'], 'Capture overlaps prior clock write')
        previous = saved
        members = row['members']
        require(members and len({m['pid'] for m in members}) == len(members), 'Duplicate/empty process family')
        family = {(m['pid'], m['started']) for m in members}
        require({(main['pid'], main['started']), (panel['pid'], panel['started'])} <= family, 'Original process missing')
        for member in members:
            require(all(type(member[k]) is int and member[k] > 0 for k in ('pid', 'started')), 'Invalid process identity')
            require(type(member['cpu_ticks']) is int and member['cpu_ticks'] >= 0, 'Invalid CPU counter')
            memory = member['memory_kib']
            require({'Pss', 'Rss', 'Private_Clean', 'Private_Dirty'} <= set(memory)
                    and all(type(v) is int and v >= 0 for v in memory.values()), 'Invalid memory counters')
            require(member['threads'] and len({t['tid'] for t in member['threads']}) == len(member['threads']), 'Thread inventory differs')
            for thread in member['threads']:
                require(all(type(thread[k]) is int and thread[k] > 0 for k in ('tid', 'started')), 'Invalid thread identity')
                require(all(type(thread[k]) is int and thread[k] >= 0 for k in ('voluntary_ctxt_switches', 'nonvoluntary_ctxt_switches')), 'Invalid switch counters')
        require(row['pss_kib'] == sum(m['memory_kib']['Pss'] for m in members), 'PSS sum differs')
        require(row['uss_kib'] == sum(sum(m['memory_kib'].get(k, 0) for k in ('Private_Clean', 'Private_Dirty', 'Private_Hugetlb')) for m in members), 'USS sum differs')
        require(row['cpu_ticks'] == sum(m['cpu_ticks'] for m in members), 'CPU sum differs')
        require(row['context_switches'] == sum(t['voluntary_ctxt_switches'] + t['nonvoluntary_ctxt_switches'] for m in members for t in m['threads']), 'Switch sum differs')
        families.append(family)
        threads.append({(m['pid'], m['started'], t['tid'], t['started']) for m in members for t in m['threads']})
        for side in ('original_identities_before', 'original_identities_after'):
            for role, identity in (('main', main), ('panel', panel)):
                require(all(row[side][role][k] == identity[k] for k in ('pid', 'started')), 'Retained identity differs')
    require(progress['tail_due_ns'] == previous['actual_start_ns'] + 250_000_000, 'Tail deadline differs')
    require(max(progress['tail_due_ns'], previous['clock_log_end_ns']) <= progress['tail_reached_ns']
            <= progress['files_flushed_ns'] < progress['deadline_ns'], 'Tail/flush ordering differs')
    require(progress['files_flushed_ns'] - progress['started_ns'] >= 240_000_000_000
            and rows[-1]['monotonic_ns'] - rows[0]['monotonic_ns'] >= 239_750_000_000, 'Capture duration differs')
    equal(metric['actual_elapsed_seconds'], (progress['files_flushed_ns'] - progress['started_ns']) / 1e9)
    equal(metric['sample_span_seconds'], (rows[-1]['monotonic_ns'] - rows[0]['monotonic_ns']) / 1e9)
    for key in ('pss_kib', 'uss_kib'):
        equal(metric[key], summary([row[key] for row in rows]), key)
    equal(metric['observer_cost_ms'], summary([row['observer_cost_ns'] / 1e6 for row in rows]))
    equal(metric['late_ms'], summary([clock['late_ns'] / 1e6 for clock in progress['clocks']]))
    for name, start, end in (('serialization_cost_ms', 'capture_end_ns', 'serialization_end_ns'),
                             ('sample_write_cost_ms', 'serialization_end_ns', 'sample_write_end_ns'),
                             ('clock_log_cost_ms', 'sample_write_end_ns', 'clock_log_end_ns')):
        equal(metric[name], summary([(c[end] - c[start]) / 1e6 for c in progress['clocks']]))
    equal(metric['observation_intervals_ms'], summary([(b['monotonic_ns'] - a['monotonic_ns']) / 1e6 for a, b in zip(rows, rows[1:])]))
    require(all(family == families[0] for family in families) and metric['stable_process_membership'] is True, 'This cohort requires stable process membership')
    stable_threads = all(t == threads[0] for t in threads)
    require(metric['stable_thread_membership'] is stable_threads, 'Thread stability differs')
    equal(metric['observed_thread_context_switch_delta'], rows[-1]['context_switches'] - rows[0]['context_switches'] if stable_threads else None)
    require(metric['wakeup_count'] is None, 'Context switches are not wakeups')
    first, last = rows[0]['cpu_counter_read_window_ns'], rows[-1]['cpu_counter_read_window_ns']
    delta, hz = rows[-1]['cpu_ticks'] - rows[0]['cpu_ticks'], metric['cpu_clock_ticks_per_second']
    low, high, twice = last[0] - first[1], last[1] - first[0], sum(last) - sum(first)
    require(delta >= 0 and type(hz) is int and hz > 0 and 0 < low <= twice / 2 <= high, 'CPU normalization invalid')
    cpu = metric['cpu_counter_normalization']
    for key, value in {'counter_delta_ticks': delta, 'interval_min_ns': low, 'interval_max_ns': high, 'twice_nominal_interval_ns': twice}.items():
        equal(cpu[key], value)
    equal(cpu['nominal_percent_one_core'], delta * 2e11 / (hz * twice))
    equal(cpu['read_window_normalization_percent_range'], [delta * 1e11 / (hz * high), delta * 1e11 / (hz * low)])
    equal(metric['cpu_percent_one_core'], cpu['nominal_percent_one_core'])


def verify(path):
    with zipfile.ZipFile(path) as archive:
        manifest, originals = read_archive(archive)
        def included(record):
            entry = originals[record['path']]
            equal({k: entry[k] for k in ('bytes', 'sha256')}, {k: record[k] for k in ('bytes', 'sha256')})
            return entry
        def original(record):
            return json.loads(archive.read(included(record)['archive_path']))
        full = original(manifest['comparison'])
        require(manifest['comparison']['sha256'] == COMPARISON_SHA and full['source_freeze']['sha256'] == FREEZE_SHA, 'Frozen source identity differs')
        freeze = original(full['source_freeze'])
        for item in [*freeze['runtime'], freeze['plan'], freeze['assessor'], freeze['assessor_tests'], freeze['qa']['log'], full['summarizer']]:
            included(item)
        omitted = {item['name']: {k: item[k] for k in ('path', 'bytes', 'sha256')} for item in manifest['omitted_elf_inputs']}
        require(set(omitted) == {'desktop', 'helper', 'private_ss_helper', 'webkit_driver', 'python', 'screenshot_tool', 'paste_tool'}, 'Omitted ELF roles differ')
        for name, item in full['inputs'].items():
            if name in omitted:
                equal(omitted[name], item, 'Omitted input identity only')
            else:
                included(item)
        require(full['source_revision'] == REVISION and all(full[k] is False for k in ('performance_eligible', 'phase6_passed', 'phase7_authorized')), 'Scope flags differ')
        groups = {r: {'latencies': [], 'states': {s: {'pss': [], 'uss': [], 'metrics': []} for s in ('visible', 'hidden')}} for r in ('tauri', 'mygo')}
        require([a['renderer'] for a in full['arms_in_execution_order']] == ['tauri', 'mygo', 'mygo', 'tauri'], 'ABBA order differs')
        for arm in full['arms_in_execution_order']:
            assessment = original(arm['assessment'])
            execution = original(assessment['execution'])
            included(execution['pure_pacer'])
            original(arm['independent_raw_review'])
            for item in arm['raw_evidence']:
                entry = originals[item['path']]
                require(entry['role'] == 'raw', 'Raw role differs')
                equal({k: entry[k] for k in ('bytes', 'sha256')}, {k: item[k] for k in ('bytes', 'sha256')})
            prefix = 'raw/' + arm['arm'] + '/functional/'
            report = json.loads(archive.read(prefix + 'performance.json'))
            require(report['status'] == 'passed' and report['source_revision'] == REVISION
                    and report['renderer'] == arm['renderer'] and len(report['warmup']) == 5 and len(report['samples']) == 30, 'Arm identity/count differs')
            first = report['first_hotkey']
            for sample in report['samples']:
                require(all(sample[k] == first[k] for k in ('pid', 'started', 'xid', 'session_id')), 'Latency original identity differs')
                require(sample['native']['active'] == first['xid'] and first['xid'] in sample['native']['focus_chain'], 'Native endpoint differs')
                equal(sample['activation_proxy_ms'], (sample['native_observed_ns'] - sample['input']['input_start_ns']) / 1e6)
            values = [sample['activation_proxy_ms'] for sample in report['samples']]
            equal(report['warm_activation_proxy_ms'], summary(values))
            equal(arm['warm_activation_proxy_ms'], summary(values))
            groups[arm['renderer']]['latencies'].extend(values)
            for state in ('visible', 'hidden'):
                rows = [json.loads(line) for line in archive.read(prefix + 'resources-' + state + '.jsonl').splitlines()]
                clocks = [json.loads(line) for line in archive.read(prefix + 'resource-clock-' + state + '.jsonl').splitlines()]
                metric = report['resources'][state]
                check_resource(rows, clocks, report['resource_sampling_progress'][state], metric, report['original_main'], first)
                equal(assessment['verified_resources'][state], metric)
                equal(arm['resources'][state], metric)
                group = groups[arm['renderer']]['states'][state]
                group['pss'].extend(row['pss_kib'] for row in rows)
                group['uss'].extend(row['uss_kib'] for row in rows)
                group['metrics'].append(metric)
        computed = {}
        for renderer, group in groups.items():
            require(len(group['latencies']) == 60, 'Pooled latency count differs')
            computed[renderer] = {'warm_activation_proxy_ms': summary(group['latencies']), 'resources': {}}
            for state, values in group['states'].items():
                require(len(values['pss']) == len(values['uss']) == 1920, 'Pooled memory count differs')
                clocks = [m['cpu_counter_normalization'] for m in values['metrics']]
                hz_values = {m['cpu_clock_ticks_per_second'] for m in values['metrics']}
                require(len(hz_values) == 1, 'CPU clock frequency differs')
                hz = hz_values.pop()
                ticks = sum(c['counter_delta_ticks'] for c in clocks)
                twice = sum(c['twice_nominal_interval_ns'] for c in clocks)
                low, high = sum(c['interval_min_ns'] for c in clocks), sum(c['interval_max_ns'] for c in clocks)
                cpu = {'counter_delta_ticks': ticks, 'ticks_per_second': hz, 'sum_twice_nominal_intervals_ns': twice,
                       'nominal_percent_one_core': ticks * 2e11 / (hz * twice),
                       'read_window_normalization_percent_range': [ticks * 1e11 / (hz * high), ticks * 1e11 / (hz * low)],
                       'scope': 'Duration-weighted disjoint first/last counter-read intervals. Not elapsed-window/tail CPU, a common-time family bound, tick-quantization bound or statistical confidence interval.'}
                computed[renderer]['resources'][state] = {'empirical_snapshot_pss_kib': summary(values['pss']),
                    'empirical_snapshot_uss_kib': summary(values['uss']), 'cpu_counter_interval': cpu}
        equal(computed, full['variants'], 'frozen aggregate')
        return {'status': 'offline_statistics_and_inventory_match', 'source_revision': REVISION,
                'included_files': len(manifest['files']), 'original_raw_files': 136, 'formal_latencies': 120,
                'resource_captures': 7680, 'resource_clock_rows': 7680, 'omitted_elf_inputs': 7,
                'comparison_sha256': COMPARISON_SHA, 'variants': computed,
                'scope': 'Recomputed numeric/clock consistency and included-byte hashes only. Does not rerun native acceptance, isolation, visual review, or hash omitted ELF bytes.',
                'performance_eligible': False, 'phase6_passed': False, 'phase7_authorized': False}


if __name__ == '__main__':
    require(len(sys.argv) == 2, 'Usage: python verify-linux-count-v1.py phase6-linux-count-v1-inputs.zip')
    print(json.dumps(verify(sys.argv[1]), indent=2))
