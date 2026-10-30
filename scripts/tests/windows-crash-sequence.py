"""Five predeclared fresh-profile diagnostic cases; imports and pure scheduling do no native work."""
import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid


SPEC = importlib.util.spec_from_file_location('sequence_collector', Path(__file__).with_name('windows-crash-diagnostics.py'))
COLLECTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COLLECTOR)
REPOSITORY = COLLECTOR.REPOSITORY
CASES = tuple('fresh-profile-' + str(number) for number in range(1, 6))
OUTPUT_LIMIT = 8192
METADATA_FILE_LIMIT = 16 * 1024 * 1024
METADATA_CASE_LIMIT = 128 * 1024 * 1024
SOURCE_PATHS = (
    'scripts/tests/windows-crash-sequence.py', 'scripts/tests/windows-crash-diagnostics.py',
    'scripts/tests/windows-crash-diagnostics.ps1', 'scripts/tests/native-windows-surface.mjs',
    'scripts/tests/native-windows-surface-target.py', 'scripts/tests/native-windows-clipboard.py',
    'scripts/tests/native-windows-crash-events.mjs', 'scripts/build-surface.mjs',
    'scripts/release-windows-upgrade.mjs', 'scripts/mygo-surface-patch.mjs',
    'scripts/generate-surface-tokens.mjs', '.github/workflows/ci.yml',
)
require = COLLECTOR.require


def same_path(left, right):
    return os.path.normcase(os.path.abspath(left)) == os.path.normcase(os.path.abspath(right))


def read_output_tail(path, offset):
    path = COLLECTOR.plain_path(path)
    size = path.stat().st_size
    require(type(offset) is int and 0 <= offset <= size and size - offset <= OUTPUT_LIMIT,
            'Collector output tail exceeds 8 KiB or output was truncated')
    with path.open('rb') as stream:
        stream.seek(offset)
        data = stream.read(OUTPUT_LIMIT + 1)
    require(len(data) == size - offset and path.stat().st_size == size, 'Collector outputs changed while reading')
    return data


def parse_outputs(data):
    require(len(data) <= OUTPUT_LIMIT, 'Collector output tail exceeds 8 KiB')
    result = {}
    for line in data.decode('utf-8', errors='strict').splitlines():
        require('=' in line, 'Invalid collector output line')
        name, value = line.split('=', 1)
        require(name in ('root', 'functional_root', 'accepted_dump') and name not in result,
                'Unknown or duplicate collector output')
        require(value and not any(ord(character) < 32 for character in value), 'Invalid collector output value')
        result[name] = value
    require('root' in result and 'functional_root' in result, 'Missing collector roots')
    return result


def bind_roots(outputs, temporary, previous):
    require(Path(outputs['root']).is_absolute() and Path(outputs['functional_root']).is_absolute(), 'Collector roots must be absolute')
    temporary = COLLECTOR.plain_path(temporary, directory=True)
    root = COLLECTOR.plain_path(outputs['root'], temporary, directory=True)
    require(root.parent == temporary and re.fullmatch(COLLECTOR.ROOT_PATTERN, root.name), 'Unexpected collector root')
    nonce = root.name.removeprefix('dezirclip-crash-diagnostics-')
    functional = Path(os.path.abspath(outputs['functional_root']))
    require(functional == temporary / ('dezirclip-surface-' + nonce), 'Functional root is not this case\'s fresh profile')
    require(not any(same_path(root, value) for value in previous), 'A previous collector root was reused')
    if functional.exists():
        COLLECTOR.plain_path(functional, temporary, directory=True)
    if 'accepted_dump' in outputs:
        accepted = COLLECTOR.plain_path(outputs['accepted_dump'], root)
        require(accepted == root / 'verified' / 'original-desktop.dmp', 'Unexpected accepted dump path')
    return root, functional


def copy_metadata(root, functional, destination):
    """Copy only top-level metadata; never descend into profiles, candidates or dumps."""
    inventory, total = [], 0
    for source_root, group, suffixes in ((root, 'wer', ('.json',)), (functional, 'functional', ('.json', '.log', '.png'))):
        if not source_root.exists():
            continue
        COLLECTOR.plain_path(source_root, directory=True)
        target_root = destination / group
        target_root.mkdir()
        for source in sorted(source_root.iterdir()):
            if source.suffix.lower() not in suffixes:
                continue
            require(len(inventory) < 256, 'Too many top-level metadata files')
            source = COLLECTOR.plain_path(source, source_root)
            before = metadata_digest(source)
            total += before['bytes']
            require(total <= METADATA_CASE_LIMIT, 'Case metadata exceeds 128 MiB')
            target = target_root / source.name
            copied = 0
            with source.open('rb') as original, target.open('xb') as output:
                while chunk := original.read(65536):
                    copied += len(chunk)
                    require(copied <= before['bytes'], 'Metadata grew during copying')
                    output.write(chunk)
            require(metadata_digest(source) == before and metadata_digest(target) == before, 'Metadata copy changed bytes')
            inventory.append({'source': str(source), 'copy': str(target), **before})
    return inventory


def metadata_digest(path):
    path = COLLECTOR.plain_path(path)
    if path.stat().st_size == 0:
        return {'bytes': 0, 'sha256': hashlib.sha256(b'').hexdigest()}
    return COLLECTOR.digest_file(path, METADATA_FILE_LIMIT)


def bind_success(diagnostics, report, journal, workflow, root, functional, fixed_inputs, collector_exit_code):
    """Zero alone never permits the next case; every original receipt must agree."""
    require(type(collector_exit_code) is int and collector_exit_code == 0, 'Original collector failed')
    require(diagnostics.get('schema_version') == 1 and diagnostics.get('kind') == 'windows_release_crash_diagnostics',
            'Invalid diagnostics receipt')
    require(diagnostics.get('workflow') == workflow and journal.get('workflow') == workflow,
            'Diagnostics or journal belongs to another workflow')
    require(same_path(diagnostics.get('root', ''), root) and same_path(journal.get('root', ''), root)
            and same_path(diagnostics.get('functional_root', ''), functional), 'Receipt root disagreement')
    require(type(diagnostics.get('exit_code')) is int and diagnostics['exit_code'] == 0
            and type(diagnostics.get('functional_exit_code')) is int and diagnostics['functional_exit_code'] == 0,
            'Original functional or collector exit did not pass')
    require('diagnostics_error' not in diagnostics, 'Collector recorded an exception')
    cleanup = diagnostics.get('cleanup') or {}
    require(cleanup.get('status') == 'restored' and journal.get('cleanup') == cleanup
            and journal.get('created_child') is True, 'Owned WER cleanup was not confirmed restored')
    require(report.get('schema_version') == 1 and report.get('kind') == 'phase6_windows_native_functional'
            and all(report.get(key) == value for key, value in workflow.items()), 'Functional report identity mismatch')
    require(report.get('status') == 'passed' and report.get('cleanup_errors') == [] and not report.get('error'),
            'Original functional report failed')
    verdict = report.get('desktop_exit_verdict') or {}
    require(verdict.get('allowed') is True and verdict.get('abnormal_before_cleanup') is False,
            'Original exit verdict is not explicitly allowed')
    collection = diagnostics.get('collection') or {}
    require(collection.get('reproduction_status') == 'not_reproduced' and collection.get('candidates') == []
            and 'accepted' in collection and collection['accepted'] is None, 'A crash candidate or incomplete collection stops the sequence')
    require(collection.get('original_functional_status') == report['status']
            and collection.get('original_exit_verdict') == verdict and collection.get('original_functional_error') is None,
            'Collection and original functional verdict disagree')
    for name, path in (('functional_report', functional / 'report.json'),
                       ('original_process_receipt', functional / 'desktop.process.json')):
        receipt = collection.get(name) or {}
        require(same_path(receipt.get('path', ''), path)
                and {key: receipt.get(key) for key in ('bytes', 'sha256')} == COLLECTOR.digest_file(path, COLLECTOR.MAX_METADATA_READ),
                'Collection metadata hash does not bind the original receipt')
    require(len(fixed_inputs) == 3, 'Exactly three fixed binary/proof inputs are required')
    expected = [dict(source=item['path'], path=str(functional / destination), bytes=item['bytes'], sha256=item['sha256'])
                for item, destination in zip(fixed_inputs, ('candidate/dezirclip.exe', 'candidate/dzc-surface.exe', 'helper.proof.json'))]
    require(report.get('input_bytes') == expected, 'Original inputs differ from the fixed sequence build')
    integrity = report.get('input_integrity_after') or {}
    require(integrity.get('unchanged') is True and integrity.get('copied_originals') == [
        {key: item[key] for key in ('source', 'path', 'sha256')} for item in expected], 'Original input integrity was not confirmed')
    for item in expected:
        path = COLLECTOR.plain_path(item['path'], functional)
        require(COLLECTOR.digest_file(path, 256 * 1024 * 1024) == {key: item[key] for key in ('bytes', 'sha256')},
                'Actual candidate bytes changed after the collector')
    return True


def input_snapshot(paths):
    return [{'path': str(COLLECTOR.plain_path(path)), **COLLECTOR.digest_file(path, 256 * 1024 * 1024)} for path in paths]


def stop_sequence(manifest, error, persist=None, item=None):
    code = next((entry['collector_exit_code'] for entry in reversed(manifest['cases'])
                 if type(entry.get('collector_exit_code')) is int and entry['collector_exit_code'] != 0), 1)
    if item is not None:
        item.update(status='stopped', qualified=False, error=str(error), finished_at=COLLECTOR.utc_now())
        manifest['stopped_case'] = item['id']
    manifest.update(status='stopped_failure', error=str(error), exit_code=code, finished_at=COLLECTOR.utc_now())
    print('Crash sequence stopped: ' + str(error)[:2048], file=sys.stderr)
    if persist is not None:
        try:
            persist(manifest)
        except BaseException as secondary:
            manifest['persistence_error'] = str(secondary)
            print('Crash sequence metadata persistence failed; retained exit code ' + str(code), file=sys.stderr)
    return code


def run_cases(manifest, execute, persist):
    """Injectable success-only loop. Any failed launch, receipt or persistence stops dispatch."""
    require([item['id'] for item in manifest['cases']] == list(CASES), 'Only the five predeclared cases are permitted')
    require(all(item['status'] == 'not_started' for item in manifest['cases']), 'Cases cannot be resumed or retried')
    for item in manifest['cases']:
        item.update(status='started', started_at=COLLECTOR.utc_now(), qualified=False)
        try:
            persist(manifest)
            qualified = execute(item)
            require(type(item.get('collector_exit_code')) is int and item['collector_exit_code'] == 0,
                    'Original collector did not exit successfully')
            require(qualified is True, 'Case was not fully qualified')
            item.update(status='not_reproduced', qualified=True, finished_at=COLLECTOR.utc_now())
            persist(manifest)
        except BaseException as error:
            return stop_sequence(manifest, error, persist, item)
    manifest.update(status='not_reproduced_5', exit_code=0, finished_at=COLLECTOR.utc_now())
    try:
        persist(manifest)
    except BaseException as error:
        return stop_sequence(manifest, error, persist)
    return 0


def run(args, workflow):
    temporary = COLLECTOR.plain_path(os.environ['RUNNER_TEMP'], directory=True)
    sequence_root = temporary / ('dezirclip-crash-sequence-' + uuid.uuid4().hex)
    manifest = {'schema_version': 1, 'kind': 'windows_release_crash_sequence', 'workflow': workflow,
                'root': str(sequence_root), 'started_at': COLLECTOR.utc_now(), 'status': 'preparing',
                'maximum_cases': 5, 'cases': [{'id': name, 'status': 'not_started'} for name in CASES],
                'limits': ['Diagnostic only; no resource or ABBA measurement.',
                           'Five successes mean not_reproduced_5, never a fix or a closed historical failure.',
                           'No failed case is repeated, replaced or skipped; only confirmed success permits the next case.',
                           'Each original collector creates one fresh profile and restores WER before the next case.']}
    manifest_path = sequence_root / 'sequence.json'
    owned = False

    def persist(value):
        COLLECTOR.write_json(manifest_path, value, replace=manifest_path.exists())

    try:
        require(not sequence_root.exists(), 'Preserve existing sequence evidence')
        COLLECTOR.private_directory(sequence_root)
        COLLECTOR.plain_path(sequence_root, temporary, directory=True)
        owned = True
        COLLECTOR.emit_output('sequence_root', str(sequence_root))
        persist(manifest)
        revision = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=REPOSITORY, capture_output=True, text=True, check=True).stdout.strip()
        require(revision == workflow['source_revision'], 'Sequence must use the exact workflow checkout')
        binaries = input_snapshot([Path(os.path.abspath(value)) for value in (args.desktop, args.helper, args.helper_proof)])
        sources = input_snapshot([REPOSITORY / value for value in SOURCE_PATHS])
        proof = COLLECTOR.read_json(args.helper_proof)
        require(proof.get('expected', {}).get('source_revision') == revision, 'Helper proof belongs to another source revision')
        proof_paths = []
        for item in proof.get('inputs', []):
            path = COLLECTOR.plain_path(REPOSITORY / item['path'], REPOSITORY)
            require(COLLECTOR.digest_file(path, 256 * 1024 * 1024)['sha256'] == item['sha256'], 'Helper source hash mismatch')
            proof_paths.append(path)
        require(proof_paths, 'Helper proof must freeze its original source inputs')
        helper_sources = input_snapshot(proof_paths)
        manifest.update(status='running', fixed_inputs=binaries, source_inputs=sources, helper_source_inputs=helper_sources)
        persist(manifest)
        previous = set()
        output_path = COLLECTOR.plain_path(os.environ['GITHUB_OUTPUT'])

        def execute(item):
            paths = [entry['path'] for entry in binaries + sources + helper_sources]
            expected = binaries + sources + helper_sources
            item['inputs_before'] = input_snapshot(paths)
            require(item['inputs_before'] == expected, 'Fixed input bytes changed before this case')
            destination = sequence_root / item['id']
            destination.mkdir()
            offset = COLLECTOR.plain_path(output_path).stat().st_size
            item['github_output_offset'] = offset
            command = [sys.executable, '-B', str(REPOSITORY / 'scripts/tests/windows-crash-diagnostics.py'), 'run',
                       '--desktop', args.desktop, '--helper', args.helper, '--helper-proof', args.helper_proof]
            item.update(command=command, child_launch_attempted=True)
            persist(manifest)
            # The child inherits the original GITHUB_OUTPUT so always-cleanup tracks the only active WER transaction.
            item['collector_exit_code'] = subprocess.run(command, cwd=REPOSITORY, check=False).returncode
            persist(manifest)
            data = read_output_tail(output_path, offset)
            output_copy = destination / 'collector.outputs.txt'
            output_copy.write_bytes(data)
            item['collector_outputs'] = {'path': str(output_copy), **metadata_digest(output_copy)}
            outputs = parse_outputs(data)
            item['outputs'] = outputs
            COLLECTOR.write_json(destination / 'outputs.json', outputs)
            root, functional = bind_roots(outputs, temporary, previous)
            previous.add(str(root))
            item['metadata'] = copy_metadata(root, functional, destination)
            item['inputs_after'] = input_snapshot(paths)
            require(item['inputs_after'] == expected, 'Fixed input bytes changed during this case')
            require('accepted_dump' not in outputs, 'An accepted dump stops the sequence')
            diagnostics = COLLECTOR.read_json(destination / 'wer' / 'diagnostics.json')
            report = COLLECTOR.read_json(destination / 'functional' / 'report.json')
            journal = COLLECTOR.read_json(destination / 'wer' / 'registry-journal.json')
            return bind_success(diagnostics, report, journal, workflow, root, functional, binaries, item['collector_exit_code'])

        return run_cases(manifest, execute, persist)
    except BaseException as error:
        return stop_sequence(manifest, error, persist if owned else None)


def main():
    parser = argparse.ArgumentParser()
    for name in ('desktop', 'helper', 'helper-proof'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    return run(args, COLLECTOR.context(os.environ))


if __name__ == '__main__':
    sys.exit(main())
