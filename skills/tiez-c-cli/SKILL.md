---
name: tiez-c
description: tiez-clipboard CLI for LLM agents — query, search, modify clipboard history via 12 subcommands
---

# tiez-c — tiez-clipboard CLI Agent Skill

`tiez-c` is the official command-line interface for tiez-clipboard. It exposes clipboard history as structured data so agents, scripts, and humans can read, write, search, export, and watch the local clipboard database without launching the GUI.

The CLI talks directly to the same SQLite backend the desktop app uses. There is no daemon, no REST layer, and no network hop between `tiez-c` and the data.

## When to use this skill

Use `tiez-c` when the user asks to access or manipulate clipboard history from the command line.

- User asks to access or search clipboard history
- User wants to add, delete, pin, or tag clipboard entries
- User wants to export/import clipboard data for backup or migration
- User wants to monitor new clipboard entries (watch)
- User wants to integrate clipboard with other tools (fzf, scripts, AI agents)
- User needs programmatic JSON output for downstream processing

Do NOT use this skill for:

- GUI interaction (launching the desktop window, clicking buttons)
- Direct system clipboard access (use the OS clipboard API or language bindings instead)
- Reading or writing to active applications (that is outside tiez-clipboard's scope)

## Quick reference

The 12 subcommands split cleanly into three categories.

### READ

| Command | Description | Key flags |
|---------|-------------|-----------|
| `list` | Show recent clipboard entries | `--limit N`, `--ids`, `--json`, `--quiet` |
| `search` | Find entries matching a pattern | `--mode exact/regex/fuzzy`, `--limit N` |
| `get` | Fetch a single entry by ID | `--json`, `--raw` |
| `stats` | Show counts, size, pin status | `--json` |

### WRITE

| Command | Description | Key flags |
|---------|-------------|-----------|
| `add` | Add text or a file as a new entry | `-` for stdin, `--tag`, `--pin` |
| `delete` | Permanently remove an entry | `--id`, `--yes` |
| `pin` | Pin an entry to the top of the list | `--id`, `--yes` |
| `unpin` | Remove pin from an entry | `--id`, `--yes` |
| `tag` | Add or remove tags | `add <id> <tag>`, `remove <id> <tag>` |

### WATCH

| Command | Description | Key flags |
|---------|-------------|-----------|
| `watch` | Stream new entries as they arrive | `--pattern`, `--notify`, `--json` |
| `export` | Dump database to a `.tiez` file | `--encrypted`, `--passphrase` |
| `import` | Restore from a `.tiez` file | `--mode merge/replace`, `--passphrase` |

Run `tiez-c <command> --help` for per-command options.

## Output conventions

LLMs parsing `tiez-c` output must handle several formats. Default output is human-readable, not machine-readable.

- Default output uses unicode icons (for example: 📋 🌐 🖼️ 📁). Strip these before parsing.
- `--json` returns a JSON array. Iterate all entries; do not assume a single object.
- `--ids` returns one ID per line, no header row.
- `--quiet` returns exit 0 with no output on success. Check the exit code, not stdout.
- Exit codes: `0` success, `1` user error, `2` auth or access error, `3` database corruption, `130` SIGINT.
- stderr is reserved for errors; stdout is reserved for data. Do not mix them.

## 8 Common Patterns

### Pattern 1: List recent 20 entries

Show the most recent clipboard entries in human-readable form.

```bash
tiez-c list
```

### Pattern 2: Find specific content

Search with a regular expression across titles and bodies.

```bash
tiez-c search "regex pattern" --mode regex
```

### Pattern 3: FZF interactive picker

Pipe IDs into `fzf`, then fetch the selected entry as JSON.

```bash
tiez-c list --ids | fzf | xargs tiez-c get --json
```

### Pattern 4: Bulk tag

Tag every listed entry with a single label.

```bash
tiez-c list --ids | xargs -I {} tiez-c tag add {} web
```

### Pattern 5: Watch for secrets

Monitor new clipboard entries for passwords or API keys.

```bash
tiez-c watch --pattern "password|api[_-]?key" --notify
```

### Pattern 6: Export encrypted backup

Save a portable encrypted backup.

```bash
tiez-c export /tmp/backup.tiez --encrypted --passphrase "..."
```

### Pattern 7: Restore from backup

Merge a backup into the existing database.

```bash
tiez-c import /tmp/backup.tiez --mode merge
```

### Pattern 8: Programmatic access

Extract just the content field from every entry.

```bash
tiez-c list --json | jq '.[].content'
```

## Safety & permissions

`tiez-c` is designed to be safe by default and dangerous only when explicitly invoked.

- Zero network: `tiez-c` makes no network calls. All data is local SQLite.
- Read-only by default: `list`, `search`, `get`, and `stats` never modify the database.
- Destructive actions require confirmation: `delete`, `pin`, `unpin`, `tag`, `add`, and `import` will prompt for confirmation unless `--yes` is passed.
- Irreversible actions: `delete` removes entries permanently with no soft delete. `import --mode replace` overwrites all existing data. Confirm with the user before running either.
- Encryption: `--passphrase` is a CLI argument and is visible in `ps` and process listings. For sensitive automation, pass the passphrase via stdin, a file descriptor, or an environment variable instead of a plaintext flag.

## Common errors

- `database is locked`: another `tiez-c` or tiez instance is accessing the same database file. Wait for it to close or stop the other process.
- `entry not found`: the supplied ID does not exist. Use `tiez-c list --ids` to discover valid IDs.
- `wrong passphrase`: the encrypted backup file cannot be decrypted with the supplied passphrase. Verify the passphrase and retry.
- `version mismatch`: the backup was created by an incompatible tiez version. Update or rollback tiez-clipboard to match.
- `permission denied`: the data path is not writable. Check the `TIEZ_DB_PATH` environment variable and filesystem permissions.

## Environment variables

| Variable | Purpose |
|----------|---------|
| `TIEZ_DB_PATH` | Override the default SQLite database path |
| `TIEZ_CONFIG_DIR` | Override the config directory |
| `TIEZ_NO_NOTIFY=1` | Disable desktop notifications in `watch` mode |
| `TIEZ_EDITOR` | External editor invoked by `add -` (stdin mode) |

## See also

- `docs/cli.md` — full user manual
- `skills/tiez-c-cli/examples/*.md` — five worked examples
- `tiez-c <command> --help` — per-command help
