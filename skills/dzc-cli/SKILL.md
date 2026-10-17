---
name: dzc
description: dezirclip CLI for LLM agents — query, search, modify clipboard history via 15 subcommands
---

# dzc — dezirclip CLI Agent Skill

`dzc` is the official command-line interface for dezirclip. It exposes clipboard history as structured data so agents, scripts, and humans can read, write, search, export, and watch the local clipboard database without launching the GUI.

The CLI talks directly to the same SQLite backend the desktop app uses. There is no daemon, no REST layer, and no network hop between `dzc` and the data. It works while the app is not running.

## When to use this skill

Use `dzc` when the user asks to access or manipulate clipboard history from the command line.

- User asks to access or search clipboard history
- User wants to add, edit, delete, clear, pin, or tag clipboard entries
- User wants to export/import clipboard data for backup or migration
- User wants to monitor new clipboard entries (watch)
- User wants to integrate clipboard with other tools (fzf, scripts, AI agents)
- User needs programmatic JSON output for downstream processing

Do NOT use this skill for:

- GUI interaction (launching the desktop window, clicking buttons)
- Direct system clipboard access (use the OS clipboard API or language bindings instead)
- Reading or writing to active applications (that is outside dezirclip's scope)

## Quick reference

The 15 subcommands split cleanly into three categories (`tag` adds three nested subcommands of its own).

### READ

| Command | Description | Key flags |
|---------|-------------|-----------|
| `list [N]` | Show recent entries (N defaults to 20, `-1` = all) | `[N]`, `--kind`, `--tag`, `--pinned`, `--json`, `--ids`, `-q` |
| `search <QUERY>` | Find entries matching a query | `--mode contains/fuzzy/regex/fts5`, `--limit N`, `--json`, `-q` |
| `get <ID\|latest>` | Print one entry | `--preview`, `--json`, `-q` |
| `stats` | Totals, counts by type, most-used entries | `--top N`, `--json` |
| `count` | Print the total entry count | `--json` |

### WRITE

| Command | Description | Key flags |
|---------|-------------|-----------|
| `add <CONTENT>` | Add text, stdin (`-`), or a file (`@<path>`) | `--type`, no `--tag`/`--pin` |
| `edit <ID> <CONTENT>` | Replace an entry's content and preview | `-q` |
| `delete <ID>` | Permanently remove one entry | `-y/--yes` **required** |
| `clear` | Delete unprotected history (pinned and tagged entries are kept) | `--yes` **required**, `-q` |
| `pin <ID>` | Pin an entry | `-q` |
| `unpin <ID>` | Unpin an entry | `-q` |
| `tag` | Add / remove / list tags | `add <id> <tag>`, `remove <id> <tag>`, `list <id>` |

### WATCH

| Command | Description | Key flags |
|---------|-------------|-----------|
| `watch` | Stream new entries as they arrive | `--pattern` (substring), `--interval MS`, `--json`, `-q` |
| `export <PATH>` | Dump history to a file | `--encrypted`, `--passphrase`, `-q` |
| `import <PATH>` | Restore from a file | `--mode merge/replace`, `--passphrase`, `-q` |

Run `dzc <command> --help` for per-command options.

## Output conventions

LLMs parsing `dzc` output must handle several formats. Default output is human-readable, not machine-readable.

- Entry ids are integers and `timestamp` is epoch **milliseconds**.
- Default human line is `<pin><icon> [type] <preview>` (📌 prefix when pinned, icons 📋 🌐 🖼️ 📁 💻). Strip them before parsing.
- `--json` on `list` / `search` returns a JSON **array**; on `get` / `stats` a single object; `watch --json` emits newline-delimited objects. Iterate all entries; do not assume a single object.
- `--ids` (`list` only) returns one numeric id per line, no header row.
- `--quiet` returns exit 0 with no output on success. Check the exit code, not stdout.
- Exit codes: `0` success, `1` runtime error (missing flag such as `--yes`, unknown entry id, DB/IO failure), `2` usage error from argument parsing (unknown flag, unknown subcommand, invalid value), `130` SIGINT from `watch` (Ctrl-C). No other codes are returned.
- stderr is reserved for errors and the `watch` startup banner; stdout is reserved for data. Do not mix them.

## 10 Common Patterns

### Pattern 1: List recent 20 entries

Show the most recent clipboard entries in human-readable form.

```bash
dzc list
```

### Pattern 2: Find specific content

Search with a regular expression.

```bash
dzc search "regex pattern" --mode regex
```

### Pattern 3: Count entries

Print the entry count without parsing output.

```bash
dzc count
dzc count --json      # {"count": 1234}
```

### Pattern 4: FZF interactive picker

Pipe IDs into `fzf`, then fetch the selected entry.

```bash
dzc list --ids -- -1 | fzf | xargs dzc get --json
```

### Pattern 5: Bulk tag

Tag every listed entry with a single label.

```bash
dzc list --ids -- -1 | xargs -I {} dzc tag add {} web
```

### Pattern 6: Edit an entry

Replace the content of an existing entry; the preview is re-derived as the first 500 characters.

```bash
dzc edit 142 "corrected text"
dzc edit 142 @/tmp/replacement.txt
```

### Pattern 7: Watch for secrets

Monitor new clipboard entries for one secret-like substring.

```bash
dzc watch --pattern "api_key" --json
```

### Pattern 8: Export encrypted backup

Save a portable encrypted backup.

```bash
dzc export /tmp/backup.dzc --encrypted --passphrase "..."
```

### Pattern 9: Restore from backup

Merge a backup into the existing database.

```bash
dzc import /tmp/backup.dzc --mode merge --passphrase "..."
```

### Pattern 10: Programmatic access

Extract just the content field from every entry.

```bash
dzc list --json -- -1 | jq '.[].content'
```

## Safety & permissions

`dzc` is designed to be safe by default and dangerous only when explicitly invoked.

- Zero network: `dzc` makes no network calls. All data is local SQLite.
- Read-only by default: `list`, `search`, `get`, `stats`, and `count` never modify the database.
- No prompting: `dzc` never asks for confirmation. `delete <ID>` and `clear` **refuse** unless `--yes` is passed; every other write runs immediately.
- Irreversible actions: `delete` removes an entry permanently with no soft delete. `clear` removes every entry that is neither pinned nor tagged. `import --mode replace` clears the table first. Confirm with the user before running any of the three.
- Encryption: `--passphrase` is a CLI argument and is visible in `ps` and process listings. For sensitive automation, resolve the passphrase through your own secret store and interpolate it in the shell rather than hard-coding it in the command.

## Common errors

- `database is locked`: another `dzc` or dezirclip instance is writing to the same database file. Wait for it to finish.
- `get: no entry for id '…'` / `tag: no entry for id=…` / `pin: no entry with id=…`: the supplied ID does not exist. Use `dzc list --ids` to discover valid IDs.
- `import: decrypt failed: …`: the encrypted backup cannot be decrypted with the supplied passphrase. Verify the passphrase and retry.
- `import: invalid mode '…' (expected 'merge' or 'replace')`: `--mode` accepts only `merge` or `replace`.
- `delete: refusing to delete id=… without --yes`: `delete` and `clear` require an explicit `--yes`.
- `permission denied`: the data path is not writable. Check the `DEZIRCLIP_DB_PATH` environment variable and filesystem permissions.

## Environment variables

| Variable | Purpose |
|----------|---------|
| `DEZIRCLIP_DB_PATH` | Override the SQLite database path |
| `TIEZ_DB_PATH` | Legacy alias for `DEZIRCLIP_DB_PATH`, honored when the first is unset |

These two are the only variables `dzc` reads. There is no config-directory, editor, or notification variable.

## See also

- `docs/cli.md` — full user manual
- `skills/dzc-cli/examples/*.md` — five worked examples
- `dzc <command> --help` — per-command help
