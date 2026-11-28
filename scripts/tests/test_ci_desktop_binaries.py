import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts import ci_desktop_binaries


class DesktopCompanionGuardTests(unittest.TestCase):
    def test_real_command_can_write_other_outputs_without_replacing_companions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = "aarch64-pc-windows-msvc"
            directory = root / "target" / target / "debug"
            directory.mkdir(parents=True)
            suffix = ".exe" if os.name == "nt" else ""
            for name in ("dzc", "dzc-service"):
                (directory / (name + suffix)).write_text(name, encoding="utf-8")
            with patch.object(ci_desktop_binaries, "ROOT", root):
                ci_desktop_binaries.protect_companions(
                    [sys.executable, "-c", "from pathlib import Path; Path('desktop').write_bytes(b'app')"], target
                )
            self.assertEqual((root / "desktop").read_bytes(), b"app")

    def test_guard_rejects_replacement_of_either_companion(self):
        for changed in ("dzc", "dzc-service"):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                directory = root / "target" / "debug"
                directory.mkdir(parents=True)
                suffix = ".exe" if os.name == "nt" else ""
                for name in ("dzc", "dzc-service"):
                    (directory / (name + suffix)).write_text(name, encoding="utf-8")
                script = "import pathlib, sys; pathlib.Path(sys.argv[1]).write_bytes(b'replaced')"
                command = [sys.executable, "-c", script, str(directory / (changed + suffix))]
                with patch.object(ci_desktop_binaries, "ROOT", root), self.assertRaisesRegex(RuntimeError, changed):
                    ci_desktop_binaries.protect_companions(command, "")


if __name__ == "__main__":
    unittest.main()
