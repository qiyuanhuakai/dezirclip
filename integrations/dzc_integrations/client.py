"""One bounded, shell-free subprocess boundary shared by all adapters."""

import json
import os
import shutil
import subprocess
import tempfile
import unicodedata


class AdapterError(Exception):
    """An expected error suitable for displaying without clipboard contents."""


TEXT_TYPES = frozenset(("text", "code", "url", "html", "rich_text"))
SENSITIVE_TAGS = frozenset(("sensitive", "密码"))


def display_text(value, limit=300):
    """Remove terminal controls and flatten payloads before showing picker rows."""
    value = str(value)
    clean = "".join(
        " " if char.isspace() else char
        for char in value
        if not unicodedata.category(char).startswith("C") or char.isspace()
    )
    return " ".join(clean.split())[:limit]


def sensitive(entry):
    tags = entry.get("tags", [])
    return (
        bool(entry.get("sensitive") or entry.get("encrypted") or entry.get("is_sensitive")
             or entry.get("is_encrypted"))
        or any(str(tag).casefold() in SENSITIVE_TAGS for tag in tags)
        or any(str(entry.get(key, "")).startswith(("dpapi:", "linux:"))
               for key in ("content", "preview", "html_content"))
    )


def bounded_int(value, name, lower, upper):
    if isinstance(value, bool) or not isinstance(value, int) or not lower <= value <= upper:
        raise AdapterError(f"{name} must be an integer from {lower} to {upper}")
    return value


class DzcClient:
    def __init__(self, executable=None):
        self.executable = executable or os.environ.get("DZC_BIN") or shutil.which("dzc")
        if not self.executable:
            raise AdapterError("dzc is not on PATH; pass --dzc with its executable path")

    def run(self, args, payload=None, max_bytes=4 * 1024 * 1024):
        # Temporary files prevent an unexpectedly large record from exhausting RAM.
        with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
            try:
                result = subprocess.run(
                    [self.executable, *args], input=payload, stdout=output, stderr=errors,
                    timeout=30, check=False,
                )
            except (OSError, subprocess.TimeoutExpired) as exc:
                raise AdapterError("Could not run dzc; check its path and database access") from exc
            if result.returncode:
                raise AdapterError(f"dzc exited with code {result.returncode}; run it directly for details")
            if output.tell() > max_bytes:
                raise AdapterError("dzc output exceeds the adapter's size limit")
            output.seek(0)
            return output.read()

    def json(self, args):
        try:
            return json.loads(self.run(args))
        except (UnicodeDecodeError, ValueError) as exc:
            raise AdapterError("dzc returned invalid JSON; update dzc and these adapters together") from exc

    def entries(self, query="", limit=100, offset=0, kind=None, exclude_sensitive=False):
        bounded_int(limit, "limit", 1, 200)
        bounded_int(offset, "offset", 0, 1_000_000)
        if query:
            args = ["search", "--json", "--metadata", "--limit", str(limit),
                    "--offset", str(offset)]
        else:
            args = ["list", "--json", "--metadata", "--offset", str(offset)]
        if kind:
            args += ["--kind", kind]
        if exclude_sensitive:
            args += ["--exclude-sensitive"]
        args += ["--", query if query else str(limit)]
        entries = self.json(args)
        if not isinstance(entries, list) or not all(isinstance(row, dict) for row in entries):
            raise AdapterError("dzc returned an unexpected history format")
        return [row for row in entries
                if not (exclude_sensitive and sensitive(row))]

    def entry(self, entry_id, exclude_sensitive=False):
        bounded_int(entry_id, "id", 1, 2**63 - 1)
        args = ["get", str(entry_id), "--json"]
        if exclude_sensitive:
            args += ["--exclude-sensitive"]
        entry = self.json(args)
        if not isinstance(entry, dict):
            raise AdapterError("dzc returned an unexpected entry format")
        if exclude_sensitive and sensitive(entry):
            raise AdapterError("This entry is unavailable to the MCP adapter")
        return entry

    def raw(self, entry_id):
        bounded_int(entry_id, "id", 1, 2**63 - 1)
        return self.run(["get", str(entry_id), "--raw"])

    def copy(self, entry_id):
        bounded_int(entry_id, "id", 1, 2**63 - 1)
        self.run(["copy", str(entry_id), "--quiet"])

    def copy_osc52(self, entry_id, passthrough="none"):
        bounded_int(entry_id, "id", 1, 2**63 - 1)
        if passthrough not in ("none", "tmux"):
            raise AdapterError("OSC 52 passthrough must be none or tmux")
        self.run(["copy", str(entry_id), "--target", "osc52",
                  "--osc52-passthrough", passthrough, "--quiet"])


def row_text(entry):
    entry_id = bounded_int(entry.get("id"), "id", 1, 2**63 - 1)
    pin = "*" if entry.get("pinned") else " "
    kind = display_text(entry.get("type", "text"), 30)
    preview = display_text(entry.get("preview", ""))
    return f"{entry_id}\t{pin} [{kind}] {preview}"
