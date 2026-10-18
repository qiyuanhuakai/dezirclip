import sys
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from dzc_integrations.__main__ import parse_args
from dzc_integrations.client import AdapterError, DzcClient
from dzc_integrations.picker import emit_selection


class Osc52AdapterTests(unittest.TestCase):
    def test_explicit_osc52_selection_uses_cli_terminal_writer(self):
        client = DzcClient("dzc")
        with patch.object(client, "run") as run:
            emit_selection(client, {"id":42,"type":"text"}, "osc52", "tmux")
        self.assertEqual(run.call_args.args[0], ["copy","42","--target","osc52",
                                              "--osc52-passthrough","tmux","--quiet"])

    def test_cancel_and_nontext_records_never_write_terminal_or_native_clipboard(self):
        client = Mock()
        self.assertEqual(emit_selection(client,None,"osc52"),1)
        for kind in ("image","file","video"):
            with self.assertRaises(AdapterError):
                emit_selection(client,{"id":42,"type":kind},"osc52")
        client.copy.assert_not_called()
        client.copy_osc52.assert_not_called()

    def test_parser_defaults_remain_print_and_never_guess_ssh(self):
        self.assertEqual(parse_args(["pick"]).action,"print")
        self.assertEqual(parse_args(["tui","--action","osc52","--osc52-passthrough","tmux"]).osc52_passthrough,"tmux")
        client = DzcClient("dzc")
        with patch.object(client,"run") as run:
            with self.assertRaises(AdapterError):
                client.copy_osc52(42,"unknown")
            run.assert_not_called()
