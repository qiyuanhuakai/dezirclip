"""Offline transport and readiness regressions; no application is launched."""
import importlib.util
import io
import json
from pathlib import Path
import unittest
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('memory_probe', Path(__file__).with_name('release-memory-windows.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class Socket:
    def __init__(self, rows, visible):
        self.rows, self.visible, self.closed = rows, visible, False

    def send(self, value):
        self.request = json.loads(value)

    def recv(self):
        return json.dumps({'id': 1, 'result': {'result': {'value': {'rows': self.rows, 'images': 2, 'visible': self.visible}}}})

    def close(self):
        self.closed = True


class Readiness(unittest.TestCase):
    def test_visible_history_and_empty_or_hidden_pages(self):
        for rows, visibility, accepted in [(3, 'visible', True), (0, 'visible', False), (3, 'hidden', False)]:
            socket = Socket(rows, visibility)
            targets = io.BytesIO(json.dumps([{'type': 'page', 'url': 'http://tauri.localhost/', 'webSocketDebuggerUrl': 'ws://127.0.0.1:9222/devtools/page/1'}]).encode())
            with patch.object(probe.HTTP, 'open', return_value=targets), patch.object(probe, 'websocket', SimpleNamespace(create_connection=lambda *args, **kwargs: socket)):
                if accepted:
                    self.assertEqual(probe.ready(9222)['rows'], rows)
                else:
                    with self.assertRaisesRegex(RuntimeError, 'History not rendered'):
                        probe.ready(9222)
            self.assertTrue(socket.closed)
            self.assertIn('[data-test-clipboard-item]', socket.request['params']['expression'])


if __name__ == '__main__':
    unittest.main()
