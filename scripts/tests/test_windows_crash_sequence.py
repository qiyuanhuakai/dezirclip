"""Standard-library fake scheduling and receipt checks; no child or Windows API execution."""
from contextlib import redirect_stderr
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).with_name('windows-crash-sequence.py')
SPEC = importlib.util.spec_from_file_location('windows_crash_sequence', SOURCE)
SEQUENCE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SEQUENCE)
WORKFLOW = {'source_revision': 'a' * 40, 'run_id': '123', 'run_attempt': '1'}


def manifest():
    return {'cases': [{'id': name, 'status': 'not_started'} for name in SEQUENCE.CASES]}


def save(path, value):
    path.write_text(json.dumps(value), encoding='utf-8')


def case_receipts(temporary, number, inputs):
    nonce = f'{number:032x}'
    root = temporary / ('dezirclip-crash-diagnostics-' + nonce)
    functional = temporary / ('dezirclip-surface-' + nonce)
    root.mkdir()
    functional.mkdir()
    (functional / 'candidate').mkdir()
    destinations = ('candidate/dezirclip.exe', 'candidate/dzc-surface.exe', 'helper.proof.json')
    copied = []
    for item, name in zip(inputs, destinations):
        path = functional / name
        path.write_bytes(Path(item['path']).read_bytes())
        copied.append(dict(source=item['path'], path=str(path), bytes=item['bytes'], sha256=item['sha256']))
    report = {'schema_version': 1, 'kind': 'phase6_windows_native_functional', **WORKFLOW,
              'status': 'passed', 'cleanup_errors': [], 'input_bytes': copied,
              'desktop_exit_verdict': {'allowed': True, 'abnormal_before_cleanup': False,
                                       'exit_code_hex': '0x00000001', 'reason': 'recorded_cleanup_termination'},
              'input_integrity_after': {'unchanged': True, 'helper_source_inputs_checked': 1,
                                       'copied_originals': [{key: item[key] for key in ('source', 'path', 'sha256')} for item in copied]}}
    cleanup = {'status': 'restored', 'created_parent': False, 'parent_retained': True, 'finished_at': '2026-10-30T00:00:00+00:00'}
    journal = {'schema_version': 1, 'workflow': copy.deepcopy(WORKFLOW), 'root': str(root),
               'created_child': True, 'cleanup': copy.deepcopy(cleanup)}
    save(functional / 'report.json', report)
    save(functional / 'desktop.process.json', {'pid': 1772})
    (functional / 'native-target.stderr.log').write_bytes(b'')
    (functional / 'native-target.stdout.log').write_bytes(b'synthetic transcript\n')
    (functional / 'frame.png').write_bytes(b'synthetic PNG metadata copy fixture')
    collection = {'reproduction_status': 'not_reproduced', 'candidates': [], 'accepted': None,
                  'original_functional_status': 'passed', 'original_functional_error': None,
                  'original_exit_verdict': copy.deepcopy(report['desktop_exit_verdict'])}
    for key, name in (('functional_report', 'report.json'), ('original_process_receipt', 'desktop.process.json')):
        path = functional / name
        collection[key] = {'path': str(path), **SEQUENCE.COLLECTOR.digest_file(path)}
    diagnostics = {'schema_version': 1, 'kind': 'windows_release_crash_diagnostics',
                   'workflow': copy.deepcopy(WORKFLOW), 'root': str(root), 'functional_root': str(functional),
                   'exit_code': 0, 'functional_exit_code': 0, 'cleanup': copy.deepcopy(cleanup), 'collection': collection}
    save(root / 'diagnostics.json', diagnostics)
    save(root / 'registry-journal.json', journal)
    return root, functional, diagnostics, report, journal


class LoopTests(unittest.TestCase):
    def run_loop(self, execute, persist=lambda value: None):
        state = manifest()
        with redirect_stderr(io.StringIO()):
            code = SEQUENCE.run_cases(state, execute, persist)
        return code, state

    def test_first_third_and_fifth_failure_stop_without_retry_or_replacement(self):
        for failed in (1, 3, 5):
            calls = []

            def execute(item):
                calls.append(item['id'])
                item['collector_exit_code'] = 19 if len(calls) == failed else 0
                return True

            with self.subTest(failed=failed):
                code, state = self.run_loop(execute)
                self.assertEqual(code, 19)
                self.assertEqual(calls, list(SEQUENCE.CASES[:failed]))
                self.assertEqual(state['status'], 'stopped_failure')
                self.assertEqual(state['cases'][failed - 1]['status'], 'stopped')
                self.assertTrue(all(item['status'] == 'not_started' for item in state['cases'][failed:]))

    def test_zero_without_explicit_qualification_never_advances(self):
        for qualification in (False, None, 1):
            calls = []

            def execute(item):
                calls.append(item['id'])
                item['collector_exit_code'] = 0
                return qualification

            code, state = self.run_loop(execute)
            self.assertEqual((code, len(calls)), (1, 1))
            self.assertFalse(state['cases'][0]['qualified'])

    def test_five_successes_only_mean_not_reproduced(self):
        calls = []

        def execute(item):
            calls.append(item['id'])
            item['collector_exit_code'] = 0
            return True

        code, state = self.run_loop(execute)
        self.assertEqual((code, calls, state['status']), (0, list(SEQUENCE.CASES), 'not_reproduced_5'))
        self.assertTrue(all(item['qualified'] is True for item in state['cases']))
        self.assertEqual(len(state['cases']), 5)

    def test_exception_and_secondary_persistence_failures_retain_original_exit(self):
        for original_code in (7, 0xc0000374):
            calls = []

            def persist(value):
                if calls:
                    raise OSError('synthetic storage failure')

            def execute(item):
                calls.append(item['id'])
                item['collector_exit_code'] = original_code
                persist(None)

            code, state = self.run_loop(execute, persist)
            self.assertEqual((code, len(calls)), (original_code, 1))
            self.assertEqual(state['exit_code'], original_code)
            self.assertIn('persistence_error', state)

    def test_copy_or_receipt_exception_retains_nonzero_exit(self):
        def execute(item):
            item['collector_exit_code'] = 13
            raise OSError('synthetic metadata copy failure')

        code, state = self.run_loop(execute)
        self.assertEqual(code, 13)
        self.assertEqual(state['stopped_case'], SEQUENCE.CASES[0])

    def test_initial_persistence_failure_launches_nothing(self):
        calls = []

        def persist(value):
            raise OSError('synthetic storage failure')

        code, state = self.run_loop(lambda item: calls.append(item), persist)
        self.assertEqual((code, calls), (1, []))
        self.assertEqual(state['status'], 'stopped_failure')

    def test_final_persistence_failure_does_not_report_success(self):
        def execute(item):
            item['collector_exit_code'] = 0
            return True

        def persist(value):
            if value.get('status') == 'not_reproduced_5':
                raise OSError('synthetic final persistence failure')

        code, state = self.run_loop(execute, persist)
        self.assertEqual((code, state['status']), (1, 'stopped_failure'))

    def test_resuming_or_adding_a_sixth_case_is_rejected(self):
        for state in (manifest(), manifest()):
            if len(state['cases']) == 5:
                state['cases'][0]['status'] = 'not_reproduced'
            with self.assertRaises(ValueError):
                SEQUENCE.run_cases(state, lambda item: self.fail('must not launch'), lambda value: None)
        state = manifest()
        state['cases'].append({'id': 'sixth', 'status': 'not_started'})
        with self.assertRaises(ValueError):
            SEQUENCE.run_cases(state, lambda item: self.fail('must not launch'), lambda value: None)


class FilesAndReceiptTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.base = Path(self.scratch.name)
        self.temporary = self.base / 'runner-temp'
        self.temporary.mkdir()
        self.inputs = []
        for name in ('desktop.fake', 'helper.fake', 'proof.fake'):
            path = self.base / name
            path.write_bytes(('synthetic ' + name).encode())
            self.inputs.append({'path': str(path), **SEQUENCE.COLLECTOR.digest_file(path)})
        self.root, self.functional, self.diagnostics, self.report, self.journal = case_receipts(self.temporary, 1, self.inputs)

    def bind(self, code=0):
        return SEQUENCE.bind_success(self.diagnostics, self.report, self.journal, WORKFLOW,
                                     self.root, self.functional, self.inputs, code)

    def test_actual_collector_field_shapes_qualify(self):
        self.assertTrue(self.bind())

    def test_each_original_success_requirement_is_mandatory(self):
        mutations = (
            lambda d, r, j: d.update(functional_exit_code=1),
            lambda d, r, j: d.update(exit_code=1),
            lambda d, r, j: d.update(diagnostics_error=''),
            lambda d, r, j: d['workflow'].update(run_attempt='2'),
            lambda d, r, j: j['workflow'].update(run_attempt='2'),
            lambda d, r, j: j.update(created_child=False),
            lambda d, r, j: j['cleanup'].update(status='failed'),
            lambda d, r, j: r.update(source_revision='b' * 40),
            lambda d, r, j: r.update(status='failed'),
            lambda d, r, j: r.update(cleanup_errors=['failure']),
            lambda d, r, j: r['desktop_exit_verdict'].update(allowed=False),
            lambda d, r, j: r['desktop_exit_verdict'].update(abnormal_before_cleanup=True),
            lambda d, r, j: d['collection'].update(reproduction_status='original_abnormal_exit_observed'),
            lambda d, r, j: d['collection'].update(candidates=[{'accepted': False}]),
            lambda d, r, j: d['collection'].update(accepted={'path': 'synthetic.dmp'}),
            lambda d, r, j: d['collection'].pop('accepted'),
            lambda d, r, j: d['collection']['functional_report'].update(sha256='b' * 64),
            lambda d, r, j: r['input_bytes'][0].update(sha256='b' * 64),
            lambda d, r, j: r['input_integrity_after'].update(unchanged=False),
            lambda d, r, j: r['input_integrity_after'].update(copied_originals=[]),
        )
        original = copy.deepcopy((self.diagnostics, self.report, self.journal))
        for number, mutate in enumerate(mutations):
            with self.subTest(number=number):
                self.diagnostics, self.report, self.journal = copy.deepcopy(original)
                mutate(self.diagnostics, self.report, self.journal)
                with self.assertRaises(ValueError):
                    self.bind()

    def test_candidate_bytes_are_rehashed_after_original_receipts(self):
        (self.functional / 'candidate' / 'dezirclip.exe').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'Actual candidate bytes'):
            self.bind()

    def test_output_tail_reads_only_new_bounded_bytes(self):
        path = self.base / 'github-output'
        prefix = b'sequence_root=old\n' * 1000
        tail = b'root=a\nfunctional_root=b\n'
        path.write_bytes(prefix + tail)
        self.assertEqual(SEQUENCE.read_output_tail(path, len(prefix)), tail)
        self.assertEqual(SEQUENCE.parse_outputs(tail), {'root': 'a', 'functional_root': 'b'})
        for offset in (-1, 0, len(prefix + tail) + 1):
            with self.assertRaises(ValueError):
                SEQUENCE.read_output_tail(path, offset)
        with self.assertRaises(ValueError):
            SEQUENCE.parse_outputs(b'x' * (SEQUENCE.OUTPUT_LIMIT + 1))

    def test_output_duplicates_unknowns_missing_roots_and_invalid_utf8_fail(self):
        for data in (b'root=a\nroot=b\nfunctional_root=c\n', b'root=a\nother=b\nfunctional_root=c\n',
                     b'root=a\n', b'root=\nfunctional_root=b\n', b'root=a\nfunctional_root=\xff\n'):
            with self.subTest(data=data), self.assertRaises((ValueError, UnicodeError)):
                SEQUENCE.parse_outputs(data)

    def test_roots_must_be_absolute_fresh_direct_temp_children_with_same_nonce(self):
        outputs = {'root': str(self.root), 'functional_root': str(self.functional)}
        self.assertEqual(SEQUENCE.bind_roots(outputs, self.temporary, set()), (self.root, self.functional))
        invalid = [dict(outputs, root=self.root.name), dict(outputs, root=str(self.base)),
                   dict(outputs, functional_root=str(self.temporary / ('dezirclip-surface-' + 'f' * 32)))]
        for item in invalid:
            with self.assertRaises(ValueError):
                SEQUENCE.bind_roots(item, self.temporary, set())
        with self.assertRaisesRegex(ValueError, 'reused'):
            SEQUENCE.bind_roots(outputs, self.temporary, {str(self.root)})

    def test_metadata_keeps_empty_logs_and_never_descends_into_private_data(self):
        for parent in (self.root / 'raw-dumps', self.functional / 'profile', self.functional / 'candidate' / 'environment'):
            parent.mkdir()
            (parent / 'secret.json').write_bytes(b'synthetic forbidden subtree')
        (self.root / 'unbound.dmp').write_bytes(b'synthetic forbidden dump')
        destination = self.base / 'copies'
        destination.mkdir()
        inventory = SEQUENCE.copy_metadata(self.root, self.functional, destination)
        self.assertEqual((destination / 'functional' / 'native-target.stderr.log').read_bytes(), b'')
        self.assertFalse(any(item['source'].endswith(('.dmp', 'secret.json')) for item in inventory))
        self.assertFalse((destination / 'functional' / 'candidate').exists())
        for item in inventory:
            self.assertEqual(SEQUENCE.metadata_digest(item['copy']), {key: item[key] for key in ('bytes', 'sha256')})

    def test_metadata_limit_and_nonregular_metadata_fail_closed(self):
        destination = self.base / 'limited'
        destination.mkdir()
        with patch.object(SEQUENCE, 'METADATA_FILE_LIMIT', 1), self.assertRaises(ValueError):
            SEQUENCE.copy_metadata(self.root, self.functional, destination)
        (self.root / 'directory.json').mkdir()
        destination = self.base / 'nonregular'
        destination.mkdir()
        with self.assertRaises(ValueError):
            SEQUENCE.copy_metadata(self.root, self.functional, destination)


class FakeRunTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.base = Path(self.scratch.name)
        self.repo, self.temporary = self.base / 'repo', self.base / 'temp'
        self.repo.mkdir()
        self.temporary.mkdir()
        (self.repo / 'source.txt').write_bytes(b'synthetic source')
        self.args = SimpleNamespace(desktop=str(self.repo / 'desktop.fake'), helper=str(self.repo / 'helper.fake'),
                                    helper_proof=str(self.repo / 'helper.proof.json'))
        Path(self.args.desktop).write_bytes(b'synthetic desktop; never executed')
        Path(self.args.helper).write_bytes(b'synthetic helper; never executed')
        save(Path(self.args.helper_proof), {'expected': {'source_revision': WORKFLOW['source_revision']},
                                          'inputs': [{'path': 'source.txt', 'sha256': SEQUENCE.COLLECTOR.digest_file(self.repo / 'source.txt')['sha256']}]})
        self.output = self.base / 'github-output'
        self.output.write_bytes(b'')
        self.calls = []
        self.child_roots = []

    def run_fake(self, failed=0, storage_failure=False):
        def subprocess_fake(command, **kwargs):
            if command == ['git', 'rev-parse', 'HEAD']:
                return SimpleNamespace(stdout=WORKFLOW['source_revision'] + '\n')
            self.assertEqual(command[1:3], ['-B', str(self.repo / 'scripts/tests/windows-crash-diagnostics.py')])
            self.calls.append(command)
            inputs = SEQUENCE.input_snapshot([self.args.desktop, self.args.helper, self.args.helper_proof])
            root, functional, diagnostics, _, _ = case_receipts(self.temporary, len(self.calls), inputs)
            code = 0xc0000374 if len(self.calls) == failed else 0
            diagnostics['exit_code'] = code
            save(root / 'diagnostics.json', diagnostics)
            self.child_roots.append(root)
            with self.output.open('a', encoding='utf-8') as stream:
                stream.write('root=' + str(root) + '\nfunctional_root=' + str(functional) + '\n')
            return SimpleNamespace(returncode=code)

        original_write = SEQUENCE.COLLECTOR.write_json

        def write_fake(path, value, replace=False):
            if storage_failure and Path(path).name == 'sequence.json' and any(
                    item.get('collector_exit_code') for item in value['cases']):
                raise OSError('synthetic repeated storage failure')
            return original_write(path, value, replace=replace)

        with patch.object(SEQUENCE, 'REPOSITORY', self.repo), patch.object(SEQUENCE, 'SOURCE_PATHS', ('source.txt',)), \
                patch.object(SEQUENCE.COLLECTOR, 'private_directory', side_effect=lambda path: path.mkdir()), \
                patch.object(SEQUENCE.subprocess, 'run', side_effect=subprocess_fake), \
                patch.object(SEQUENCE.COLLECTOR, 'write_json', side_effect=write_fake), \
                patch.dict(os.environ, RUNNER_TEMP=str(self.temporary), GITHUB_OUTPUT=str(self.output)), redirect_stderr(io.StringIO()):
            return SEQUENCE.run(self.args, WORKFLOW)

    def sequence_root(self):
        return Path(self.output.read_text(encoding='utf-8').splitlines()[0].split('=', 1)[1])

    def test_full_fake_run_preserves_all_five_metadata_and_outputs(self):
        self.assertEqual(self.run_fake(), 0)
        state = json.loads((self.sequence_root() / 'sequence.json').read_text(encoding='utf-8'))
        self.assertEqual(state['status'], 'not_reproduced_5')
        self.assertEqual(len(set(self.child_roots)), 5)
        self.assertEqual(len(self.calls), 5)
        for item in state['cases']:
            self.assertEqual(item['inputs_before'], item['inputs_after'])
            folder = self.sequence_root() / item['id']
            self.assertTrue((folder / 'outputs.json').is_file())
            self.assertTrue((folder / 'wer' / 'diagnostics.json').is_file())
            self.assertEqual((folder / 'functional' / 'native-target.stderr.log').read_bytes(), b'')
            self.assertFalse((folder / 'functional' / 'candidate').exists())
            self.assertEqual(SEQUENCE.metadata_digest(item['collector_outputs']['path']),
                             {key: item['collector_outputs'][key] for key in ('bytes', 'sha256')})

    def test_real_coordinator_with_fake_child_stops_and_keeps_all_started_cases(self):
        self.assertEqual(self.run_fake(failed=3), 0xc0000374)
        self.assertEqual(len(self.calls), 3)
        state = json.loads((self.sequence_root() / 'sequence.json').read_text(encoding='utf-8'))
        self.assertEqual(state['exit_code'], 0xc0000374)
        self.assertEqual(state['cases'][3]['status'], 'not_started')
        for name in SEQUENCE.CASES[:3]:
            self.assertTrue((self.sequence_root() / name / 'wer' / 'diagnostics.json').is_file())

    def test_post_child_persistence_failure_preserves_original_exit_and_receipt(self):
        self.assertEqual(self.run_fake(failed=1, storage_failure=True), 0xc0000374)
        self.assertEqual(len(self.calls), 1)
        original = json.loads((self.child_roots[0] / 'diagnostics.json').read_text(encoding='utf-8'))
        self.assertEqual(original['exit_code'], 0xc0000374)
        self.assertIn('root=' + str(self.child_roots[0]), self.output.read_text(encoding='utf-8'))

    def test_preexisting_sequence_root_is_not_written_emitted_or_launched(self):
        existing = self.temporary / ('dezirclip-crash-sequence-' + 'f' * 32)
        existing.mkdir()
        sentinel = existing / 'sequence.json'
        sentinel.write_bytes(b'preserve prior original')
        with patch.object(SEQUENCE.uuid, 'uuid4', return_value=SimpleNamespace(hex='f' * 32)), \
                patch.dict(os.environ, RUNNER_TEMP=str(self.temporary), GITHUB_OUTPUT=str(self.output)), \
                patch.object(SEQUENCE.COLLECTOR, 'private_directory') as private, \
                patch.object(SEQUENCE.subprocess, 'run') as child, redirect_stderr(io.StringIO()):
            self.assertEqual(SEQUENCE.run(self.args, WORKFLOW), 1)
            private.assert_not_called()
            child.assert_not_called()
        self.assertEqual(sentinel.read_bytes(), b'preserve prior original')
        self.assertEqual(self.output.read_bytes(), b'')
        self.assertEqual(list(existing.iterdir()), [sentinel])

    def test_import_does_not_dispatch_or_call_private_directory(self):
        spec = importlib.util.spec_from_file_location('fresh_sequence_import', SOURCE)
        fresh = importlib.util.module_from_spec(spec)
        with patch('subprocess.run', side_effect=AssertionError('No process may be launched on import')):
            spec.loader.exec_module(fresh)
        self.assertEqual(fresh.CASES, SEQUENCE.CASES)


if __name__ == '__main__':
    unittest.main()
