"""An optional curses browser; all mutations remain explicit key actions."""

import importlib
import os
import sys
import textwrap
import unicodedata

from .client import AdapterError, display_text, row_text


def preview_lines(value, width):
    value = "".join(char for char in str(value)
                    if char in "\n\t" or not unicodedata.category(char).startswith("C"))
    result = []
    for line in value.expandtabs(4).splitlines()[:2000]:
        result.extend(textwrap.wrap(line, max(1, width)) or [""])
    return result or ["(empty)"]


class Browser:
    def __init__(self, client, query="", limit=100, action="print"):
        self.client = client
        self.query = query
        self.limit = limit
        self.action = action
        self.offset = 0
        self.cursor = 0
        self.preview_offset = 0
        self.entries = []
        self.preview = ""
        self.status = ""
        self.preview_id = None

    def refresh(self):
        self.entries = self.client.entries(self.query, self.limit, self.offset)
        self.cursor = min(self.cursor, max(0, len(self.entries) - 1))
        self.preview_id = None
        self.preview_offset = 0

    def selected(self):
        return self.entries[self.cursor] if self.entries else None

    def update_preview(self):
        entry = self.selected()
        if entry is None:
            self.preview = "No entries. Press / to change the search."
        elif entry["id"] != self.preview_id:
            self.preview_id = entry["id"]
            self.preview_offset = 0
            fetched = self.client.entry(entry["id"])
            self.preview = str(fetched.get("content", ""))[:64_000]

    def run(self, window, curses):
        curses.curs_set(0)
        window.keypad(True)
        self.refresh()
        while True:
            try:
                self.update_preview()
            except AdapterError as exc:
                self.preview = str(exc)
            self.draw(window, curses)
            try:
                key = window.get_wch()
            except curses.error:
                continue
            entry = self.selected()
            try:
                if key in ("q", "\x1b"):
                    return None
                if key in ("\n", "\r", curses.KEY_ENTER):
                    return entry
                if key in ("j", curses.KEY_DOWN):
                    self.cursor = min(self.cursor + 1, max(0, len(self.entries) - 1))
                elif key in ("k", curses.KEY_UP):
                    self.cursor = max(0, self.cursor - 1)
                elif key == curses.KEY_NPAGE or key == "]":
                    if len(self.entries) == self.limit:
                        self.offset += self.limit
                        self.cursor = 0
                        self.refresh()
                elif key == curses.KEY_PPAGE or key == "[":
                    self.offset = max(0, self.offset - self.limit)
                    self.cursor = 0
                    self.refresh()
                elif key in (curses.KEY_RIGHT, "l"):
                    self.preview_offset += 3
                elif key in (curses.KEY_LEFT, "h"):
                    self.preview_offset = max(0, self.preview_offset - 3)
                elif key == "/":
                    query = self.prompt(window, curses, "Search (empty = all): ")
                    if query is not None:
                        self.query = query
                        self.offset = self.cursor = 0
                        self.refresh()
                elif key == "r":
                    self.refresh()
                elif entry and key == "c":
                    self.client.copy(entry["id"])
                    self.status = f"Copied entry {entry['id']}"
                elif entry and key == "p":
                    operation = "unpin" if entry.get("pinned") else "pin"
                    self.client.run([operation, str(entry["id"]), "--quiet"])
                    self.refresh()
                    self.status = f"{operation}: {entry['id']}"
                elif entry and key in ("t", "u"):
                    operation = "add" if key == "t" else "remove"
                    tag = self.prompt(window, curses, f"Tag to {operation}: ")
                    if tag:
                        self.client.run(["tag", operation, str(entry["id"]), "--", tag])
                        self.refresh()
                        self.status = f"Tag {operation} complete"
                elif entry and key == "d":
                    answer = self.prompt(window, curses, f"Delete entry {entry['id']}? Type yes: ")
                    if answer == "yes":
                        self.client.run(["delete", str(entry["id"]), "--yes"])
                        self.refresh()
                        self.status = "Entry deleted"
            except AdapterError as exc:
                self.status = str(exc)

    def draw(self, window, curses):
        window.erase()
        height, width = window.getmaxyx()

        def put(y, text, selected=False):
            if 0 <= y < height:
                try:
                    window.addstr(y, 0, text[:max(0, width - 1)],
                                  curses.A_REVERSE if selected else curses.A_NORMAL)
                except curses.error:
                    pass

        put(0, f"DezirClip  search: {display_text(self.query, 80)}  offset: {self.offset}")
        if height < 12 or width < 35:
            put(1, "Enlarge terminal; q exits")
            window.refresh()
            return
        list_height = max(3, min(12, height // 3))
        start = max(0, self.cursor - list_height + 1)
        for position, entry in enumerate(self.entries[start:start + list_height]):
            put(position + 2, row_text(entry).replace("\t", "  "), start + position == self.cursor)
        border = list_height + 3
        entry = self.selected()
        tags = ", ".join(str(tag) for tag in entry.get("tags", [])) if entry else ""
        put(border, "Preview | tags: " + display_text(tags, width - 20))
        lines = preview_lines(self.preview, width - 1)
        visible = max(1, height - border - 5)
        self.preview_offset = min(self.preview_offset, max(0, len(lines) - visible))
        for position, line in enumerate(lines[self.preview_offset:self.preview_offset + visible]):
            put(border + position + 1, line)
        put(height - 3, f"Enter {self.action} | c copy | p pin | t/u tag | d delete | / search | q quit")
        put(height - 2, "Arrows/jk select | h/l scroll preview | [/]/PgUp/PgDn page | r refresh")
        put(height - 1, display_text(self.status, width - 1))
        window.refresh()

    @staticmethod
    def prompt(window, curses, label):
        height, width = window.getmaxyx()
        value = ""
        curses.curs_set(1)
        try:
            while True:
                window.move(height - 1, 0)
                window.clrtoeol()
                try:
                    window.addstr(height - 1, 0, (label + value)[-max(1, width - 1):])
                except curses.error:
                    pass
                window.refresh()
                key = window.get_wch()
                if key in ("\n", "\r", curses.KEY_ENTER):
                    return value
                if key == "\x1b":
                    return None
                if key in (curses.KEY_BACKSPACE, "\b", "\x7f"):
                    value = value[:-1]
                elif isinstance(key, str) and key.isprintable() and len(value) < 1024:
                    value += key
        finally:
            curses.curs_set(0)


def run_tui(client, query="", limit=100, action="print"):
    try:
        curses = importlib.import_module("curses")
    except ImportError as exc:
        raise AdapterError("Install integrations/requirements-tui.txt to use the TUI on Windows") from exc
    browser = Browser(client, query=query, limit=limit, action=action)
    saved_input, saved_output = os.dup(0), os.dup(1)
    try:
        # Keep terminal control bytes out of captured stdout, including shell widgets.
        sys.stdout.flush()
        input_name = "CONIN$" if os.name == "nt" else "/dev/tty"
        output_name = "CONOUT$" if os.name == "nt" else "/dev/tty"
        with open(input_name, "rb", buffering=0) as terminal_input, \
                open(output_name, "wb", buffering=0) as terminal_output:
            os.dup2(terminal_input.fileno(), 0)
            os.dup2(terminal_output.fileno(), 1)
            return curses.wrapper(browser.run, curses)
    except (OSError, curses.error) as exc:
        raise AdapterError("The TUI needs an interactive terminal with curses support") from exc
    finally:
        sys.stdout.flush()
        os.dup2(saved_input, 0)
        os.dup2(saved_output, 1)
        os.close(saved_input)
        os.close(saved_output)
