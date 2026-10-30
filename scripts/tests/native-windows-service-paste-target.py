"""Private CI Service gate; reuse the unchanged strict Surface target implementation."""
import argparse
import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import re
import sys
import threading
import time
import traceback

SOURCE = Path(__file__).with_name('native-windows-surface-target.py')
spec = importlib.util.spec_from_file_location('service_gate_surface_target', SOURCE)
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


class ServiceTarget(base.Native):
    def __init__(self, root):
        super().__init__(root)
        self.kernel.GetCurrentProcess.argtypes = []
        self.kernel.GetCurrentProcess.restype = base.W.HANDLE
        for library, name, arguments, result in (
            (self.user, 'OpenClipboard', [base.W.HWND], base.W.BOOL),
            (self.user, 'CloseClipboard', [], base.W.BOOL),
            (self.user, 'GetClipboardData', [base.W.UINT], base.W.HANDLE),
            (self.kernel, 'GlobalSize', [base.W.HGLOBAL], ctypes.c_size_t),
            (self.kernel, 'GlobalLock', [base.W.HGLOBAL], base.W.LPVOID),
            (self.kernel, 'GlobalUnlock', [base.W.HGLOBAL], base.W.BOOL),
        ):
            function = getattr(library, name)
            function.argtypes, function.restype = arguments, result
        self.service = None
        self.service_binding_attempted = False
        self.expected_bindings = set()
        self.observation_epoch = 0
        self.observation_label = None

    def bind_service(self, pid):
        base.require(not self.service_binding_attempted, 'One original Service binding required')
        self.service_binding_attempted = True
        try:
            base.require('service' in self.expected_bindings and type(pid) is int and pid > 0,
                         'Reserved original Service binding required')
            receipt = json.loads((self.root / 'service-identity.json').read_text(encoding='utf-8'))
            executable = self.root / 'candidate/dzc-service.exe'
            base.require(not executable.is_symlink() and not executable.is_junction(), 'Service candidate changed')
            base.require(receipt['pid'] == pid and re.fullmatch('[a-f0-9]{64}', receipt['sha256'])
                         and hashlib.sha256(executable.read_bytes()).hexdigest() == receipt['sha256'],
                         'Original Service bytes differ')
            build = receipt['build']
            base.require(build['component'] == 'service' and build['source_state'] == 'verified_checkout'
                         and build['source_revision'] == os.environ['GITHUB_SHA']
                         and build['target'] == 'x86_64-pc-windows-msvc'
                         and build['build_id'] == build['component_version'] + '@' + build['source_revision'],
                         'Original Service build identity differs')
            info = self.process(pid, phase='service-original-binding')
            base.require(info['running'] and Path(info['image']).resolve() == executable.resolve(),
                         'Service original image differs')
            self.service = pid
            self.validated_owned_handles[pid] = {'handle': self.handles[pid], 'identity': dict(self.identities[pid])}
            return {'role': 'service_original', 'input': receipt, 'process': info}
        except BaseException as error:
            self.binding_error = 'Service ownership validation failed: ' + str(error)
            raise

    def own_snapshot(self):
        # This process's windows are known by construction; no process handle is opened.
        self.pump_messages()
        own = self.kernel.GetCurrentProcess()
        created, exited, kernel, user = (base.W.FILETIME() for _ in range(4))
        base.require(self.kernel.GetProcessTimes(own, ctypes.byref(created), ctypes.byref(exited),
                     ctypes.byref(kernel), ctypes.byref(user)), 'Fixture identity unavailable')
        metadata = self.click_window_metadata(self.target)
        alternate = self.click_window_metadata(self.alternate)
        base.require(metadata['exists'] and metadata['visible'] and metadata['pid'] == os.getpid()
                     and metadata['thread_id'] == self.window_thread_id
                     and metadata['root_hwnd'] == int(self.target), 'Original target window changed')
        base.require(alternate['exists'] and alternate['pid'] == os.getpid()
                     and alternate['thread_id'] == self.window_thread_id
                     and alternate['root_hwnd'] == int(self.alternate), 'Original alternate window changed')
        text = ctypes.create_unicode_buffer(4096)
        self.user.GetWindowTextW(self.alternate_edit, text, len(text))
        base.require(len(self.focus_messages) < 64, 'Fixture message ring reached its bound')
        return {'target': {'hwnd': str(int(self.target)), 'root_hwnd': str(metadata['root_hwnd']),
                          'pid': os.getpid(), 'thread_id': self.window_thread_id,
                          'creation_time_100ns': str((created.dwHighDateTime << 32) | created.dwLowDateTime)},
                'target_text': self.target_text(), 'alternate_text': text.value,
                'alternate': alternate, 'alternate_edit_hwnd': int(self.alternate_edit),
                'focus': self.focus_snapshot(), 'callback_errors': list(self.callback_errors),
                'observation_epoch': self.observation_epoch, 'observation_label': self.observation_label}

    def begin_observation_epoch(self, label):
        labels = ('correct_original_target', 'stale_original_target_refused', 'generated_held_target_refused')
        base.require(0 <= self.observation_epoch < len(labels)
                     and label == labels[self.observation_epoch], 'Predeclared observation epoch order required')
        self.pump_messages()
        base.require(not self.callback_errors, 'Original fixture callback failed before epoch')
        base.require(len(self.focus_messages) < 64, 'Prior observation epoch reached its message bound')
        epoch = self.observation_epoch + 1
        document = {'previous_epoch': self.observation_epoch, 'previous_label': self.observation_label,
                    'next_epoch': epoch, 'next_label': label, 'boundary_monotonic_ns': str(time.monotonic_ns()),
                    'previous_focus': self.focus_snapshot(), 'callback_errors': list(self.callback_errors)}
        path = self.root / ('service-input-epoch-' + str(epoch) + '-boundary.json')
        base.save(path, document)
        archive = {'file': path.name, 'bytes': path.stat().st_size,
                   'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
        # The old complete ring is durable before starting a new diagnostic scope.
        self.focus_messages.clear()
        self.observation_epoch, self.observation_label = epoch, label
        return {'observation_epoch': epoch, 'observation_label': label, 'archive': archive}

    def clipboard_matches(self, expected):
        base.require(isinstance(expected, str)
                     and re.fullmatch(r'Service paste synthetic generated [a-f0-9]{32}', expected),
                     'Only the unique generated fixture marker may be compared')
        payload = expected.encode('utf-16-le') + b'\0\0'
        base.require(self.user.OpenClipboard(self.target), 'Read-only clipboard acquisition failed')
        try:
            memory = self.user.GetClipboardData(13)
            base.require(memory, 'Unicode clipboard data unavailable')
            size = self.kernel.GlobalSize(memory)
            base.require(0 < size <= 1024 * 1024, 'Clipboard allocation outside read bound')
            matches = False
            if size >= len(payload):
                address = self.kernel.GlobalLock(memory)
                base.require(address, 'Read-only clipboard lock failed')
                try:
                    # Compare only the provided synthetic length and its NUL;
                    # never scan, decode, hash, or return unrelated clipboard text.
                    matches = ctypes.string_at(address, len(payload)) == payload
                finally:
                    self.kernel.GlobalUnlock(memory)
            return {'matches_expected': matches, 'format': 'CF_UNICODETEXT',
                    'unknown_content_returned': False, 'read_bound_bytes': len(payload)}
        finally:
            base.require(self.user.CloseClipboard(), 'Read-only clipboard close failed')

    def cleanup(self):
        original_desktop = self.desktop
        service_only = original_desktop is None and self.service is not None
        expected = getattr(self, 'expected_bindings', set())
        unresolved = [role for role, pid in (('service', self.service), ('desktop', original_desktop))
                      if role in expected and pid is None]
        if unresolved:
            self.binding_error = (getattr(self, 'binding_error', '') or '') + '; Unconfirmed reserved originals: ' + ','.join(unresolved)
        # Startup failure still discovers all descendants from the authenticated
        # Service certificate. The Desktop failure/error is preserved verbatim.
        if service_only:
            self.desktop = self.service
        try:
            base.save(self.root / 'service-root-cleanup-context.json', {
                'discovery_anchor_role': 'service_original_before_desktop_bind' if service_only else 'desktop_and_service_originals',
                'actual_desktop_binding': original_desktop, 'authenticated_service_binding': self.service,
                'unresolved_reserved_originals': unresolved,
                'original_binding_error': getattr(self, 'binding_error', None)})
            result = super().cleanup()
            result['discovery_anchor_role'] = 'service_original_before_desktop_bind' if service_only else 'desktop_and_service_originals'
            result['actual_desktop_binding'] = original_desktop
            return result
        finally:
            self.desktop = original_desktop


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True)
    parser.add_argument('--source', required=True)
    parser.add_argument('--port', required=True, type=int)
    args = parser.parse_args()
    root = base.guarded_root(args.root, args.source)
    base.require(1024 <= args.port <= 65535, 'Private loopback port required')
    state = base.bootstrap(root, args.port, args.source)
    native = None
    requests = queue.Queue(maxsize=32)

    def reader():
        try:
            for line in sys.stdin:
                base.require(len(line) <= 8192, 'Bounded fixture control required')
                requests.put(json.loads(line))
        finally:
            requests.put({'id': None, 'operation': 'stop'})

    failure = None
    try:
        native = ServiceTarget(root)
        threading.Thread(target=reader, daemon=True).start()
        print(json.dumps({'event': 'ready', 'target_pid': os.getpid()}), flush=True)
        deadline = time.monotonic() + 300
        while True:
            base.require(time.monotonic() < deadline, 'Service fixture deadline expired')
            native.pump_messages()
            try:
                request = requests.get(timeout=0.01)
            except queue.Empty:
                continue
            try:
                operation = request.get('operation')
                if operation == 'stop':
                    print(json.dumps({'id': request.get('id'), 'value': {'stopping': True}}), flush=True)
                    break
                if operation in ('expect-service-original', 'expect-desktop-original'):
                    role = 'service' if operation == 'expect-service-original' else 'desktop'
                    base.require(role not in native.expected_bindings, 'One reservation per original role required')
                    native.expected_bindings.add(role)
                    value = {'reserved_original_role': role}
                elif operation == 'bind-service-original':
                    value = native.bind_service(request.get('pid'))
                elif operation == 'bind':
                    base.require('desktop' in native.expected_bindings, 'Desktop spawn reservation required')
                    value = native.bind(request.get('pid'))
                    document = state[0].read_json(state[2])[0]
                    state[0].inspect(state[1], state[2], document, request['pid'], root / 'candidate/dezirclip.exe')
                elif operation == 'snapshot':
                    value = native.own_snapshot()
                elif operation == 'begin-observation-epoch':
                    value = native.begin_observation_epoch(request.get('label'))
                elif operation == 'clipboard-matches-generated':
                    value = native.clipboard_matches(request.get('expected'))
                elif operation == 'observe':
                    value = native.observe()
                elif operation == 'activate-target':
                    value = native.activate()
                elif operation == 'activate-alternate':
                    value = native.activate(alternate=True)
                elif operation == 'target-preflight':
                    value = native.target_preflight(request.get('expected_text'))
                elif operation == 'clear-target':
                    base.require(native.target_text() == '' or native.desktop is not None,
                                 'A populated target requires its bound Desktop')
                    base.require(native.user.SetWindowTextW(native.edit, ''), 'Cannot clear the original synthetic target')
                    value = native.own_snapshot()
                else:
                    raise RuntimeError('Unknown service fixture operation')
                print(json.dumps({'id': request.get('id'), 'value': value}), flush=True)
            except BaseException as error:
                print(json.dumps({'id': request.get('id'), 'error': str(error)}), flush=True)
                raise
    except BaseException:
        failure = traceback.format_exc()
        raise
    finally:
        cleanup = {'primary_error': failure}
        try:
            cleanup['native'] = native.cleanup() if native else {'not_created': True}
        except BaseException:
            cleanup['native_error'] = traceback.format_exc()
        if 'native_error' not in cleanup:
            try:
                base.restore_bootstrap(*state, root)
            except BaseException:
                cleanup['bootstrap_error'] = traceback.format_exc()
        base.save(root / 'service-native-cleanup.json', cleanup)
        base.require('native_error' not in cleanup and 'bootstrap_error' not in cleanup, 'Original Service fixture cleanup failed')


if __name__ == '__main__':
    main()
