"""Scoped adapter to the existing WebView policy transaction for disposable NSIS QA."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_windows_webview as policy

REPOSITORY = 'qiyuanhuakai/dezirclip'
ROOT = Path(__file__).resolve().parents[2]
GUI_LABELS = {
    'OldToNew': {'upgraded'},
    'PublishedAlphaToAlpha': {'upgraded'},
    'OwnedUpgrade': {'initial', 'upgraded'},
    'OwnedExternalEdit': {'initial'},
}
RECEIPT_SUFFIXES = ('', '.configure.log', '.inspect.log', '.restore.log')
MAX_RECEIPT_BYTES = 8 * 1024 * 1024


def ci_context(source, environment, checkout):
    policy.require(environment.get('GITHUB_ACTIONS') == 'true'
        and environment.get('RUNNER_ENVIRONMENT') == 'github-hosted'
        and environment.get('GITHUB_REPOSITORY') == REPOSITORY,
        'NSIS WebView policy requires the same repository on a disposable GitHub runner')
    policy.require(isinstance(source, str) and re.fullmatch('[a-f0-9]{40}', source)
        and source == environment.get('GITHUB_SHA') == checkout,
        'NSIS WebView policy requires the actual immutable CI checkout')
    policy.require(environment.get('GITHUB_EVENT_NAME') in ('pull_request', 'push', 'workflow_dispatch')
        and all(isinstance(environment.get(name), str) and re.fullmatch('[1-9][0-9]*', environment[name])
            for name in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT')),
        'NSIS WebView policy requires the original CI event, run and attempt')
    return {'mode': 'nsis_native_ci', 'source_revision': source,
        'run_id': environment['GITHUB_RUN_ID'], 'run_attempt': environment['GITHUB_RUN_ATTEMPT'],
        'event_name': environment['GITHUB_EVENT_NAME']}


def boundaries(temporary, fixture_root, policy_root, scenario, label, port):
    policy.require(scenario in GUI_LABELS and label in GUI_LABELS[scenario] and type(port) is int and port == 9222,
        'Only an original NSIS GUI phase and its fixed CDP port may configure policy')
    temporary = Path(temporary).resolve(strict=True)
    fixture_root = policy.owned(fixture_root, temporary, exists=True)
    policy_root = policy.owned(policy_root, temporary, exists=True)
    match = re.fullmatch('dezirclip-upgrade-([a-f0-9]{32})', fixture_root.name)
    policy.require(match is not None and fixture_root.parent == temporary and policy_root.parent == temporary
        and policy_root.name == 'dezirclip-nsis-webview-' + match[1]
        and fixture_root.is_dir() and policy_root.is_dir(), 'Exact sibling scenario-owned directories required')
    return fixture_root, policy_root


class OriginalDesktopToken:
    def __init__(self, api, pid, created, executable):
        policy.require(type(pid) is int and 0 < pid < 2**32 and isinstance(created, str)
            and re.fullmatch('[1-9][0-9]{0,19}', created) and int(created) < 2**64,
            'The retained original Desktop PID and positive FILETIME are required')
        self.api, self.pid, self.created, self.executable = api, pid, int(created), executable

    def token(self, pid):
        policy.require(pid == self.pid, 'Only the retained original Desktop token may be inspected')
        value = self.api.token(pid)
        policy.require(type(value['pid']) is int and value['pid'] == self.pid and type(value['creation_time_100ns']) is int
            and value['creation_time_100ns'] == self.created
            and Path(value['executable']).resolve() == self.executable.resolve(),
            'Actual Desktop token does not bind the original image and creation time')
        return value


class Session:
    def __init__(self, temporary, fixture_root, policy_root, scenario, label, port, context):
        self.fixture_root, self.root = boundaries(temporary, fixture_root, policy_root, scenario, label, port)
        self.profile = policy.owned(self.root / 'browser', self.root)
        self.state = policy.owned(self.root / (label + '-webview-policy.json'), self.root)
        self.executable = self.fixture_root / 'installed' / 'dezirclip.exe'
        self.context, self.port = {**context, 'scenario': scenario, 'label': label}, port

    def document(self):
        document, _ = policy.read_json(policy.owned(self.state, self.root, exists=True))
        policy.validate_document(document, self.root, self.context)
        policy.require(Path(document['profile']) == self.profile and document['port'] == self.port,
            'NSIS policy profile or CDP port differs from the same scenario')
        return document

    def configure(self, api):
        self.assert_fresh()
        return policy.configure(api, self.state, self.root, self.profile, self.port, self.context)

    def assert_fresh(self):
        for suffix in RECEIPT_SUFFIXES:
            path = policy.owned(Path(str(self.state) + suffix), self.root)
            policy.require(not path.exists(), 'Every original Label policy receipt must be absent before configure')

    def inspect(self, api, pid, created):
        executable = policy.owned(self.executable, self.fixture_root, exists=True)
        policy.require(executable.is_file(), 'The original installed Desktop must remain a regular file')
        exact = OriginalDesktopToken(api, pid, created, executable)
        return policy.inspect(exact, self.state, self.document(), pid, executable)

    def restore(self, api):
        policy.restore(api, self.state, self.document())

    def preserve(self, require_complete=False):
        copied, missing = [], []
        if require_complete:
            policy.require(all(policy.owned(Path(str(self.state) + suffix), self.root).is_file()
                for suffix in RECEIPT_SUFFIXES), 'Successful GUI acceptance requires all four original policy receipts')
        for suffix in RECEIPT_SUFFIXES:
            source = policy.owned(Path(str(self.state) + suffix), self.root)
            if not source.exists():
                missing.append(source.name)
                continue
            before = source.lstat()
            policy.require(stat.S_ISREG(before.st_mode) and 0 < before.st_size <= MAX_RECEIPT_BYTES,
                'Only a bounded regular original policy receipt may be preserved')
            with source.open('rb') as stream:
                opened = os.fstat(stream.fileno())
                data = stream.read(MAX_RECEIPT_BYTES + 1)
                after = os.fstat(stream.fileno())
            policy.require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
                == (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns)
                == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
                and len(data) == before.st_size, 'Original policy receipt changed while preserving it')
            destination = policy.owned(self.fixture_root / source.name, self.fixture_root)
            with destination.open('xb') as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            policy.require(destination.read_bytes() == data, 'Preserved policy receipt differs from its original bytes')
            copied.append({'name': source.name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()})
        if require_complete:
            policy.require(not missing and len(copied) == len(RECEIPT_SUFFIXES),
                'A required original policy receipt disappeared during preservation')
        return {'copied': copied, 'not_created': missing, 'browser_profile_copied': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('fresh', 'configure', 'inspect', 'restore', 'preserve'))
    parser.add_argument('--source', required=True)
    parser.add_argument('--fixture-root', type=Path, required=True)
    parser.add_argument('--policy-root', type=Path, required=True)
    parser.add_argument('--scenario', required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--port', type=int, required=True)
    parser.add_argument('--pid', type=int)
    parser.add_argument('--created')
    parser.add_argument('--require-complete', action='store_true')
    args = parser.parse_args()
    policy.require(sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true'
        and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted', 'Only disposable Windows CI may execute this adapter')
    checkout = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    context = ci_context(args.source, os.environ, checkout)
    configuration = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text(encoding='utf-8'))
    policy.require(configuration['identifier'] == policy.NAMES[1], 'Only the fixed DezirClip policy names are permitted')
    session = Session(os.environ['RUNNER_TEMP'], args.fixture_root, args.policy_root,
        args.scenario, args.label, args.port, context)
    if args.mode in ('fresh', 'configure'):
        session.assert_fresh()
    if args.mode == 'fresh':
        print(json.dumps({'status': 'fresh_label_receipts'}))
        return
    if args.mode == 'preserve':
        print(json.dumps(session.preserve(args.require_complete)))
        return
    api = policy.NativeWindows()
    if args.mode == 'configure':
        value = session.configure(api)
        print(json.dumps({'status': 'configured', 'mode': value['mode']}))
    elif args.mode == 'inspect':
        session.inspect(api, args.pid, args.created)
        print(json.dumps({'status': 'observed_original_desktop'}))
    else:
        session.restore(api)
        print(json.dumps({'status': 'restored'}))


if __name__ == '__main__':
    main()
