import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


LAUNCHER = Path(__file__).resolve().parents[1] / "dzc-integrations.py"


class RofiCallbackTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dzc-rofi-callback-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.log = self.directory / "dzc-call.json"
        # Python executes the local `copy` script when the client invokes
        # [DZC_BIN, "copy", ID, "--quiet"], on both Windows and Linux.
        (self.directory / "copy").write_text(
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "Path(os.environ['DZC_ROFI_CALL_LOG']).write_text(\n"
            "    json.dumps(['copy', *sys.argv[1:]]), encoding='utf-8')\n",
            encoding="utf-8",
        )
        self.environment = dict(os.environ, DZC_BIN=sys.executable, ROFI_RETV="1",
                                ROFI_INFO="42", DZC_ROFI_CALL_LOG=str(self.log),
                                DEZIRCLIP_DB_PATH=str(self.directory / "unused.db"),
                                PYTHONDONTWRITEBYTECODE="1")

    def launch(self, arguments, info="42"):
        return subprocess.run(
            [sys.executable, str(LAUNCHER), *arguments], cwd=self.directory,
            env=dict(self.environment, ROFI_INFO=info), capture_output=True,
            timeout=10, check=False,
        )

    def test_selected_display_row_reaches_callback_and_only_info_selects_id(self):
        row = "999\t  [text] example 你好 $(not-executed)"
        result = self.launch(["rofi", row])
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        self.assertEqual(result.stdout, b"")
        self.assertEqual(json.loads(self.log.read_text(encoding="utf-8")), ["copy", "42", "--quiet"])

    def test_malformed_info_is_rejected_even_when_display_row_contains_valid_id(self):
        for info in ("", "0", "-1", "42; anything", "not-an-id", str(2**63)):
            with self.subTest(info=info):
                result = self.launch(["rofi", "42\t  [text] valid display row"], info=info)
                self.assertEqual(result.returncode, 1)
                self.assertFalse(self.log.exists())
                self.assertEqual(result.stdout, b"")

    def test_other_commands_and_unknown_options_still_reject_extra_arguments(self):
        for arguments in (["pick", "unexpected-row"], ["tui", "unexpected-row"],
                          ["mcp", "unexpected-row"], ["pick", "--unknown-option"],
                          ["rofi", "42\trow", "--unknown-option"]):
            with self.subTest(arguments=arguments):
                result = self.launch(arguments)
                self.assertEqual(result.returncode, 2)
                self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
