"""Verify ordinary tag receipts or the fixed alpha.2 draft recovery context."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess


VERSION = '0.2.0-alpha.2'
TAG = 'v' + VERSION
SOURCE = '037f40cf014cd4bb3dce4a0a0c5cbcaee23affae'
RELEASE_ID = 422045565
REPOSITORY = 'qiyuanhuakai/dezirclip'
WORKFLOW_REF = REPOSITORY + '/.github/workflows/release.yml@refs/heads/master'
ROOT = Path(__file__).resolve().parents[1]
ALLOWED_FILES = frozenset({
    '.github/workflows/release.yml',
    '.github/workflows/release-identities.yml',
    '.github/workflows/release-windows-upgrade.yml',
    '.github/workflows/ci.yml',
    'scripts/release_recovery.py',
    'scripts/test_release_recovery.py',
    'scripts/release_identity_inputs.py',
    'scripts/tests/test_release_identity_inputs.py',
    'scripts/release_identities.py',
    'scripts/release_identity_merge.py',
    'scripts/release-windows-upgrade.mjs',
    'scripts/test-release-windows-upgrade.ps1',
    'scripts/tests/release-windows-upgrade.node.mjs',
    'docs/specs/v0.2-alpha2-release.md',
})
RELEASE_ID_ENV = ('DZC_RELEASE_ID', 'DZC_RECEIPT_RELEASE_ID', 'DZC_UPGRADE_RELEASE_ID')
REVISION = re.compile('[a-f0-9]{40}')
POSITIVE = re.compile('[1-9][0-9]*')


def require(value, message):
    if not value:
        raise ValueError(message)


def _release_id(environment, recovery):
    supplied = [environment[name] for name in RELEASE_ID_ENV if name in environment]
    require(all(isinstance(value, str) and POSITIVE.fullmatch(value) for value in supplied),
            'Release ID environment must contain positive decimal identities')
    identities = {int(value) for value in supplied}
    require(len(identities) <= 1, 'Release ID environment aliases disagree')
    if recovery:
        require(not identities or identities == {RELEASE_ID}, 'Recovery is limited to the original alpha.2 draft release')
        return RELEASE_ID
    return next(iter(identities), None)


def _context(expected_source, environment):
    require(isinstance(expected_source, str) and REVISION.fullmatch(expected_source), 'Exact source revision required')
    require(environment.get('GITHUB_ACTIONS') == 'true'
            and environment.get('RUNNER_ENVIRONMENT') == 'github-hosted'
            and environment.get('GITHUB_REPOSITORY') == REPOSITORY,
            'Release receipts require the same repository on a disposable GitHub runner')
    recovery = environment.get('DZC_RELEASE_RECOVERY') == '1'
    tooling = environment.get('DZC_RELEASE_TOOLING_SOURCE') or expected_source
    require(isinstance(tooling, str) and REVISION.fullmatch(tooling), 'Exact workflow tooling revision required')
    if recovery:
        require(expected_source == SOURCE, 'Recovery is limited to the original alpha.2 source')
        require(environment.get('GITHUB_EVENT_NAME') == 'workflow_dispatch'
                and environment.get('GITHUB_REF') == 'refs/heads/master'
                and environment.get('GITHUB_WORKFLOW_REF') == WORKFLOW_REF,
                'Recovery requires the fixed manual release workflow on master')
        require(environment.get('DZC_RELEASE_TOOLING_SOURCE') == tooling,
                'Recovery must explicitly identify its workflow tooling source')
        require(all(isinstance(environment.get(name), str) and POSITIVE.fullmatch(environment[name])
                    for name in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT')),
                'Recovery requires its actual run and attempt identities')
    else:
        require(environment.get('GITHUB_REF') == 'refs/tags/' + TAG,
                'Ordinary receipts require the final alpha.2 tag')
        require(tooling == expected_source, 'Ordinary tag tooling must match the product source')
    require(environment.get('GITHUB_SHA') == tooling, 'Actual workflow source differs from its declared tooling source')
    return {
        'schema_version': 1,
        'mode': 'recovery' if recovery else 'tag',
        'source_revision': expected_source,
        'workflow_source_revision': tooling,
        'tag': TAG,
        'release_id': _release_id(environment, recovery),
        'run_id': environment.get('GITHUB_RUN_ID') or None,
        'run_attempt': environment.get('GITHUB_RUN_ATTEMPT') or None,
    }


def _changes(raw):
    if not raw:
        return []
    values = raw.split('\0')
    require(values[-1] == '' and (len(values) - 1) % 2 == 0, 'Malformed immutable tooling diff')
    changes = []
    for index in range(0, len(values) - 1, 2):
        header, path = values[index:index + 2]
        fields = header.removeprefix(':').split()
        require(header.startswith(':') and len(fields) == 5
                and all(REVISION.fullmatch(value) for value in fields[2:4])
                and fields[4] in ('A', 'D', 'M'), 'Unsupported immutable tooling diff entry')
        old_mode, new_mode = fields[:2]
        regular = {'100644', '100755'}
        require((fields[4] == 'A' and old_mode == '000000' and new_mode in regular)
                or (fields[4] == 'D' and old_mode in regular and new_mode == '000000')
                or (fields[4] == 'M' and old_mode in regular and new_mode in regular),
                'Recovery changes must remain ordinary CI or receipt files')
        require(path in ALLOWED_FILES, 'Recovery changes a file outside the fixed CI and receipt allowlist: ' + path)
        changes.append(path)
    require(len(changes) == len(set(changes)), 'Duplicate immutable tooling diff paths')
    return changes


def _checkout(context, head, status, tag_source=None, ancestor=False, raw_diff=''):
    require(head == context['workflow_source_revision'] and status == '', 'Workflow tooling checkout must be exact and clean')
    if context['mode'] == 'recovery':
        require(tag_source == SOURCE, 'The original alpha.2 tag no longer identifies its frozen product source')
        require(ancestor is True, 'The frozen alpha.2 source must remain an ancestor of the workflow tooling source')
        _changes(raw_diff)


def _git(root, *arguments, ancestor=False):
    environment = {key: value for key, value in os.environ.items() if not key.upper().startswith('GIT_')}
    result = subprocess.run(['git', '--no-replace-objects', '-c', 'core.fsmonitor=false', '-C', str(root), *arguments],
                            env=environment, capture_output=True, text=True, encoding='utf-8', check=False)
    if ancestor:
        require(result.returncode in (0, 1), 'Cannot inspect the immutable release ancestry')
        return result.returncode == 0
    require(result.returncode == 0, 'Cannot inspect the immutable release checkout: ' + arguments[0])
    return result.stdout


def authority(expected_source, root=None):
    context = _context(expected_source, os.environ)
    root = Path(root).resolve() if root is not None else ROOT
    head = _git(root, 'rev-parse', 'HEAD').strip()
    status = _git(root, 'status', '--porcelain=v1', '--untracked-files=no')
    if context['mode'] == 'tag':
        _checkout(context, head, status)
    else:
        tooling = context['workflow_source_revision']
        tag_source = _git(root, 'rev-parse', '--verify', 'refs/tags/' + TAG + '^{commit}').strip()
        ancestor = _git(root, 'merge-base', '--is-ancestor', SOURCE, tooling, ancestor=True)
        raw = _git(root, 'diff', '--raw', '--no-abbrev', '--no-renames', '--no-ext-diff', '--no-textconv', '-z', SOURCE, tooling, '--')
        _checkout(context, head, status, tag_source, ancestor, raw)
    return context


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['authority'])
    parser.add_argument('expected_source')
    arguments = parser.parse_args()
    print(json.dumps(authority(arguments.expected_source), separators=(',', ':')))


if __name__ == '__main__':
    main()
