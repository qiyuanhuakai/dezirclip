import argparse
import sys

from .client import AdapterError, DzcClient
from .picker import emit_selection, rofi_script, select


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description="Optional local DezirClip adapters")
    parser.add_argument("--dzc", help="Path to the dzc executable (or set DZC_BIN)")
    commands = parser.add_subparsers(dest="command", required=True)
    picker = commands.add_parser("pick", help="Select one entry with fzf, rofi or dmenu")
    picker.add_argument("--backend", choices=("fzf", "rofi", "dmenu"), default="fzf")
    picker.add_argument("--kind", help="Filter by a clipboard content type")
    tui = commands.add_parser("tui", help="Browse, preview and manage history in a terminal")
    rofi = commands.add_parser("rofi", help="Rofi script mode: copy the selected record")
    rofi.add_argument("selected_row", nargs="?",
                      help="Display row supplied by rofi; the ID is read only from ROFI_INFO")
    commands.add_parser("mcp", help="Serve read-only local history tools over MCP stdio")
    for command in (picker, tui, rofi):
        command.add_argument("--query", default="", help="Initial contains search")
        command.add_argument("--limit", type=int, choices=range(1, 201), default=100, metavar="1..200")
    for command in (picker, tui):
        command.add_argument("--action", choices=("print", "copy", "id", "insert", "osc52"), default="print",
                             help="insert prints text suitable for a shell buffer and rejects NUL bytes")
        command.add_argument("--osc52-passthrough", choices=("none", "tmux"), default="none",
                             help="Explicit terminal passthrough when --action osc52 is selected")
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    try:
        client = DzcClient(args.dzc)
        if args.command == "mcp":
            from .mcp import serve
            return serve(client, sys.stdin.buffer, sys.stdout.buffer)
        if args.command == "rofi":
            return rofi_script(client, args.query, args.limit)
        if args.command == "tui":
            from .tui import run_tui
            selected = run_tui(client, args.query, args.limit, args.action)
        else:
            selected = select(client, args.backend, args.query, args.limit, args.kind)
        return emit_selection(client, selected, args.action, args.osc52_passthrough)
    except AdapterError as exc:
        print(f"dzc adapter: {exc}", file=sys.stderr)
        return 1
    except (KeyboardInterrupt, BrokenPipeError):
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
