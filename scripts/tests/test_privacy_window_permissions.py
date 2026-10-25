"""The explicit desktop capability must permit privacy's native focus query."""
import json
import pathlib
import unittest


class PrivacyWindowPermissions(unittest.TestCase):
    def test_explicit_capability_grants_focus_query_to_privacy_windows(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        config = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
        selected = config["app"]["security"]["capabilities"]
        self.assertTrue(selected, "The explicit capability list selects the final desktop grants")
        for label in ("main", "compact-preview", "quick-paste", "region-select"):
            with self.subTest(window=label):
                self.assertTrue(any(isinstance(capability, dict)
                    and label in capability.get("windows", [])
                    and "core:window:allow-is-focused" in capability.get("permissions", [])
                    for capability in selected),
                    f"{label}'s privacy lifecycle queries native focus; a separate default.json is not selected implicitly")


if __name__ == "__main__":
    unittest.main()
