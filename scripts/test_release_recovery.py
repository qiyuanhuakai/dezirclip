"""Pure fact checks for the explicitly bounded alpha.2 recovery authority."""
import unittest
from release_recovery import (
    RELEASE_ID, SOURCE, TAG, WORKFLOW_REF, _changes, _checkout, _context,
)


TOOLING = 'b' * 40


def normal():
    return {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted',
            'GITHUB_REPOSITORY': 'qiyuanhuakai/dezirclip', 'GITHUB_REF': 'refs/tags/' + TAG,
            'GITHUB_SHA': SOURCE}


def recovery():
    return {**normal(), 'DZC_RELEASE_RECOVERY': '1', 'DZC_RELEASE_TOOLING_SOURCE': TOOLING,
            'GITHUB_SHA': TOOLING, 'GITHUB_REF': 'refs/heads/master',
            'GITHUB_EVENT_NAME': 'workflow_dispatch', 'GITHUB_WORKFLOW_REF': WORKFLOW_REF,
            'GITHUB_RUN_ID': '39984241118', 'GITHUB_RUN_ATTEMPT': '3'}


def change(path, old='100644', new='100644', status='M'):
    return f':{old} {new} {"a" * 40} {"b" * 40} {status}\0{path}\0'


class AuthorityContextTests(unittest.TestCase):
    def test_ordinary_tag_keeps_existing_minimal_context(self):
        value = _context(SOURCE, normal())
        self.assertEqual(value, {'schema_version': 1, 'mode': 'tag', 'source_revision': SOURCE,
                                'workflow_source_revision': SOURCE, 'tag': TAG, 'release_id': None,
                                'run_id': None, 'run_attempt': None})

    def test_ordinary_tag_keeps_actual_optional_run_values(self):
        environment = {**normal(), 'GITHUB_RUN_ID': '42', 'GITHUB_RUN_ATTEMPT': '2',
                       'DZC_RECEIPT_RELEASE_ID': '123'}
        value = _context(SOURCE, environment)
        self.assertEqual((value['release_id'], value['run_id'], value['run_attempt']), (123, '42', '2'))

    def test_ordinary_tag_rejects_false_authority_and_wrong_source(self):
        wrong = {'GITHUB_ACTIONS': 'false', 'RUNNER_ENVIRONMENT': 'self-hosted',
                 'GITHUB_REPOSITORY': 'someone/dezirclip', 'GITHUB_REF': 'refs/heads/master',
                 'GITHUB_SHA': TOOLING, 'DZC_RELEASE_TOOLING_SOURCE': TOOLING}
        for name, value in wrong.items():
            with self.subTest(field=name), self.assertRaises(ValueError):
                _context(SOURCE, {**normal(), name: value})
        with self.assertRaises(ValueError):
            _context('not-a-commit', normal())

    def test_recovery_requires_explicit_opt_in(self):
        environment = recovery()
        for flag in (None, '', '0', 'true'):
            candidate = dict(environment)
            if flag is None:
                candidate.pop('DZC_RELEASE_RECOVERY')
            else:
                candidate['DZC_RELEASE_RECOVERY'] = flag
            with self.subTest(flag=flag), self.assertRaises(ValueError):
                _context(SOURCE, candidate)

    def test_recovery_has_separate_actual_tooling_and_product_sources(self):
        self.assertEqual((SOURCE, TAG, RELEASE_ID, WORKFLOW_REF),
                         ('037f40cf014cd4bb3dce4a0a0c5cbcaee23affae', 'v0.2.0-alpha.2', 422045565,
                          'qiyuanhuakai/dezirclip/.github/workflows/release.yml@refs/heads/master'))
        value = _context(SOURCE, recovery())
        self.assertEqual((value['mode'], value['source_revision'], value['workflow_source_revision']),
                         ('recovery', SOURCE, TOOLING))
        self.assertEqual((value['release_id'], value['run_id'], value['run_attempt']),
                         (RELEASE_ID, '39984241118', '3'))

    def test_recovery_rejects_other_source_or_workflow_context(self):
        with self.assertRaises(ValueError):
            _context('c' * 40, recovery())
        wrong = {'GITHUB_EVENT_NAME': 'push', 'GITHUB_REF': 'refs/heads/feature',
                 'GITHUB_WORKFLOW_REF': WORKFLOW_REF.replace('release.yml', 'ci.yml'),
                 'GITHUB_SHA': SOURCE, 'DZC_RELEASE_TOOLING_SOURCE': SOURCE}
        for name, value in wrong.items():
            with self.subTest(field=name), self.assertRaises(ValueError):
                _context(SOURCE, {**recovery(), name: value})
        candidate = recovery()
        candidate.pop('DZC_RELEASE_TOOLING_SOURCE')
        with self.assertRaises(ValueError):
            _context(SOURCE, candidate)

    def test_recovery_requires_actual_positive_run_and_attempt(self):
        for name in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'):
            for value in ('', '0', '-1', 'attempt-2'):
                with self.subTest(field=name, value=value), self.assertRaises(ValueError):
                    _context(SOURCE, {**recovery(), name: value})

    def test_recovery_rejects_conflicting_release_aliases(self):
        for name in ('DZC_RELEASE_ID', 'DZC_RECEIPT_RELEASE_ID', 'DZC_UPGRADE_RELEASE_ID'):
            with self.subTest(field=name):
                self.assertEqual(_context(SOURCE, {**recovery(), name: str(RELEASE_ID)})['release_id'], RELEASE_ID)
                for value in ('', '123', 'invalid', '0422045565'):
                    with self.subTest(value=value), self.assertRaises(ValueError):
                        _context(SOURCE, {**recovery(), name: value})
        with self.assertRaises(ValueError):
            _context(SOURCE, {**normal(), 'DZC_RELEASE_ID': '1', 'DZC_UPGRADE_RELEASE_ID': '2'})


class ImmutableCheckoutTests(unittest.TestCase):
    def test_ordinary_checkout_still_requires_exact_clean_source(self):
        value = _context(SOURCE, normal())
        _checkout(value, SOURCE, '')
        for head, status in [(TOOLING, ''), (SOURCE, ' M src/App.tsx\n')]:
            with self.subTest(head=head, status=status), self.assertRaises(ValueError):
                _checkout(value, head, status)

    def test_recovery_accepts_only_clean_descendant_and_original_tag(self):
        value = _context(SOURCE, recovery())
        _checkout(value, TOOLING, '', SOURCE, True, change('.github/workflows/release.yml'))
        for head, status, tag, ancestor in [(SOURCE, '', SOURCE, True), (TOOLING, ' M x\n', SOURCE, True),
                                             (TOOLING, '', TOOLING, True), (TOOLING, '', SOURCE, False)]:
            with self.subTest(head=head, status=status, tag=tag, ancestor=ancestor), self.assertRaises(ValueError):
                _checkout(value, head, status, tag, ancestor)

    def test_reviewed_ci_and_receipt_paths_support_ordinary_file_changes(self):
        paths = ['.github/workflows/release.yml', '.github/workflows/release-identities.yml',
                 '.github/workflows/release-windows-upgrade.yml', '.github/workflows/ci.yml',
                 'scripts/release_recovery.py', 'scripts/test_release_recovery.py',
                 'scripts/release_identity_inputs.py', 'scripts/tests/test_release_identity_inputs.py',
                 'scripts/release_identities.py', 'scripts/release_identity_merge.py',
                 'scripts/release-windows-upgrade.mjs', 'scripts/test-release-windows-upgrade.ps1',
                 'scripts/tests/release-windows-upgrade.node.mjs', 'docs/specs/v0.2-alpha2-release.md']
        for path in paths:
            with self.subTest(path=path):
                self.assertEqual(_changes(change(path)), [path])
                self.assertEqual(_changes(change(path, '000000', '100644', 'A')), [path])
                self.assertEqual(_changes(change(path, '100644', '000000', 'D')), [path])

    def test_product_and_unlisted_receipt_changes_are_rejected(self):
        paths = ['src/App.tsx', 'src-tauri/tauri.conf.json', 'src-tauri/src/main.rs', 'Cargo.toml',
                 'Cargo.lock', 'crates/dzc-core/src/lib.rs', 'scripts/unreviewed-receipt.py',
                 '.github/workflows/unknown.yml', '../scripts/release_recovery.py']
        for path in paths:
            with self.subTest(path=path), self.assertRaises(ValueError):
                _changes(change(path))

    def test_rename_cannot_hide_a_deleted_product_path(self):
        renamed = change('src/App.tsx', '100644', '000000', 'D') + change('scripts/release_recovery.py', '000000', '100644', 'A')
        with self.assertRaises(ValueError):
            _changes(renamed)

    def test_symlink_submodule_and_malformed_diff_are_rejected(self):
        path = 'scripts/release_recovery.py'
        values = [change(path, '100644', '120000', 'T'), change(path, '000000', '160000', 'A'),
                  change(path) + change(path), change(path).rstrip('\0'), 'unexpected\0' + path + '\0']
        for raw in values:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                _changes(raw)


if __name__ == '__main__':
    unittest.main()
