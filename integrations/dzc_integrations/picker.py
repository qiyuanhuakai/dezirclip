"""fzf, dmenu and rofi adapters; selected contents never become shell code."""

import os
import shutil
import subprocess
import sys

from .client import AdapterError, TEXT_TYPES, row_text


def select(client, backend="fzf", query="", limit=100, kind=None):
    entries = client.entries(query=query, limit=limit, kind=kind)
    if not entries:
        return None
    executable = shutil.which(backend)
    if not executable:
        raise AdapterError(f"{backend} is not on PATH")
    rows = [row_text(entry) for entry in entries]
    commands = {
        "fzf": [executable, "--no-multi", "--delimiter=\t", "--with-nth=2..",
                "--prompt=clipboard> "],
        "dmenu": [executable, "-i", "-p", "clipboard"],
        "rofi": [executable, "-dmenu", "-i", "-p", "clipboard"],
    }
    env = os.environ.copy()
    # An inherited fzf binding or preview may run arbitrary user shell commands.
    env.pop("FZF_DEFAULT_OPTS", None)
    env.pop("FZF_DEFAULT_OPTS_FILE", None)
    try:
        result = subprocess.run(
            commands[backend], input=("\n".join(rows) + "\n").encode("utf-8"),
            stdout=subprocess.PIPE, env=env, check=False,
        )
    except OSError as exc:
        raise AdapterError(f"Could not start {backend}") from exc
    if result.returncode in (1, 130):
        return None
    if result.returncode:
        raise AdapterError(f"{backend} exited with code {result.returncode}")
    try:
        choice = result.stdout.decode("utf-8").strip("\r\n")
        index = rows.index(choice)
    except (UnicodeDecodeError, ValueError) as exc:
        raise AdapterError("The picker returned an entry outside the candidate list") from exc
    return entries[index]


def emit_selection(client, entry, action, osc52_passthrough="none"):
    if entry is None:
        return 1
    if action == "copy":
        client.copy(entry["id"])
    elif action == "osc52":
        if entry.get("type") not in TEXT_TYPES:
            raise AdapterError("OSC 52 only copies text; use native copy for images and files")
        client.copy_osc52(entry["id"], osc52_passthrough)
    elif action == "id":
        print(entry["id"])
    else:
        if entry.get("type") not in TEXT_TYPES:
            raise AdapterError("Use --action copy for images and files")
        content = client.raw(entry["id"])
        if action == "insert" and b"\0" in content:
            raise AdapterError("Shell editing buffers cannot represent NUL bytes; use --action copy")
        sys.stdout.buffer.write(content)
        sys.stdout.buffer.flush()
    return 0


def rofi_script(client, query="", limit=100):
    """Rofi script mode uses numeric IDs as hidden per-row info."""
    stage = os.environ.get("ROFI_RETV", "0")
    if stage == "0":
        print("\0prompt\x1fDezirClip")
        print("\0no-custom\x1ftrue")
        for entry in client.entries(query=query, limit=limit):
            print(f"{row_text(entry)}\0info\x1f{entry['id']}")
        return 0
    if stage == "1":
        value = os.environ.get("ROFI_INFO", "")
        if not value.isdecimal() or int(value) <= 0:
            raise AdapterError("rofi did not return a valid entry ID")
        client.copy(int(value))
        return 0
    return 1
