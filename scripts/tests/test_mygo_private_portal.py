import runpy
import unittest
from unittest.mock import patch
from pathlib import Path


fixture = runpy.run_path(str(Path(__file__).with_name('mygo-desktop-x11.py')))
mounted = fixture['private_portal_mounted']


class PrivatePortalTests(unittest.TestCase):
    def setUp(self):
        self.home = Path('/tmp/dzc-mygo-private fixture')

    def mount(self, path, kind='fuse.portal'):
        escaped = str(path).replace(' ', r'\040')
        return f'7 5 0:6 / {escaped} rw,nosuid,nodev - {kind} portal rw'

    def test_only_the_exact_private_portal_is_selected(self):
        unrelated = self.mount(Path('/run/user/1000/doc'))
        sibling = self.mount(self.home / 'runtime/doc-other')
        own = self.mount(self.home / 'runtime/doc')
        self.assertFalse(mounted(self.home, unrelated + '\n' + sibling))
        self.assertTrue(mounted(self.home, unrelated + '\n' + sibling + '\n' + own))

    def test_non_portal_filesystems_are_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'Unexpected filesystem'):
            mounted(self.home, self.mount(self.home / 'runtime/doc', 'ext4'))

    def test_absent_mount_needs_no_unmount(self):
        self.assertFalse(mounted(self.home, ''))

    def test_context_detaches_only_its_portal_before_removing_profile(self):
        detached = False
        portal = None

        def read_mounts(path):
            self.assertEqual(path, Path('/proc/self/mountinfo'))
            return '' if detached else self.mount(portal)

        def detach(command, **options):
            nonlocal detached
            self.assertEqual(command, ['fixture-fusermount', '-u', '-z', str(portal)])
            self.assertEqual(options['timeout'], 10)
            self.assertTrue(portal.parent.is_dir())
            detached = True
            return fixture['subprocess'].CompletedProcess(command, 0, '', '')

        with patch.object(Path, 'read_text', autospec=True, side_effect=read_mounts), \
                patch.object(fixture['shutil'], 'which', return_value='fixture-fusermount'), \
                patch.object(fixture['subprocess'], 'run', side_effect=detach) as run:
            with self.assertRaisesRegex(ValueError, 'original test failure'):
                with fixture['private_desktop_directory']() as path:
                    home = Path(path).resolve(strict=True)
                    (home / 'runtime').mkdir()
                    portal = home / 'runtime/doc'
                    raise ValueError('original test failure')
            run.assert_called_once()
        self.assertTrue(detached)
        self.assertFalse(home.exists())


if __name__ == '__main__':
    unittest.main()
