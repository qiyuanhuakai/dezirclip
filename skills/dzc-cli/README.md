# dzc-cli skill — Developer Notes

The skill follows the current source tree. **Native `pick`, `tui`, `mcp`, and Shell initialization commands are development additions after `v0.1.20-alpha.1`; the published alpha.1 does not include them.** Check `dzc --help`, use a build containing these commands, and keep skill documentation aligned with the executable version. The native integrations require no Python runtime.

## Layout

```
skills/dzc-cli/
├── SKILL.md              # Skill manifest (commands, parameters, examples)
├── install.sh            # Linux installer (symlink)
├── install.ps1           # Windows installer (junction)
├── README.md             # This file
└── examples/
    ├── bulk-tag.md       # Bulk-tag entries matching a filter
    ├── export-encrypted.md # Encrypted backup / restore workflow
    ├── fzf-pick.md       # Native TUI / fzf picker and exact text output
    ├── search-and-add.md # Example workflow: search + add pattern
    └── watch-trigger.md  # Watch stream piped into a matcher
```

This directory ships inside the app bundle (`resources: ["../skills/dzc-cli"]`) and is also what the installers below link into an agent's skills directory.

## Installing for end users

These scripts install the **skill documentation**, not the `dzc` binary. Use the desktop sidecar or the real standalone CLI archive from [GitHub Releases](https://github.com/qiyuanhuakai/dezirclip/releases). See [the CLI manual](../../docs/cli.md) for PATH, database coexistence, and release compatibility.

**Linux:**
```bash
bash skills/dzc-cli/install.sh
```

**Windows (PowerShell):**
```powershell
powershell -ExecutionPolicy Bypass -File skills/dzc-cli/install.ps1
```

**Uninstall:**
```bash
bash skills/dzc-cli/install.sh --uninstall
```
```powershell
powershell -ExecutionPolicy Bypass -File skills/dzc-cli/install.ps1 --uninstall
```

## Updating the skill

This skill uses a **symlink/junction model** for installation.

- **Symlink model (default):** The installer creates a symlink (or junction on Windows) from `~/.claude/skills/dzc-cli` back to the source directory in the repository. To update, pull the latest changes in the repo — no re-install needed because the symlink always points to the current tree.
- **Copy model:** If you instead copy the skill directory manually into `~/.claude/skills/`, re-run the installer after pulling updates to refresh the copy.

## Native entrypoints

The same executable now contains the terminal UI, external-picker bridge, Shell widgets, and read-only MCP service. These are post-alpha.1 commands:

```bash
dzc pick                              # Built-in TUI; Enter copies natively
dzc tui --query project --action print # Exact selected text, no added newline
dzc pick --backend fzf --action id     # Optional installed fzf
dzc pick --backend rofi                # Optional installed Rofi dmenu mode
dzc mcp                               # MCP client launches this over stdio
```

`--backend dmenu` is also available. No external picker is required by default. Native clipboard actions need an accessible desktop session; in SSH use `--action osc52` explicitly for terminal-mediated local text copying. This does not synchronize history.

For Ctrl-G insertion into the editing buffer, load `eval "$(dzc bash)"`, `eval "$(dzc zsh)"`, or `dzc fish | source` in the appropriate Shell. PowerShell 7 with PSReadLine uses `dzc pwsh | Out-String | Invoke-Expression`. Nushell 0.116+ uses an explicit file:

```nu
dzc nu --install ($nu.user-autoload-dirs.0 | path join 'dzc.nu')
```

Restart Nushell after installing into autoload. A different existing file is preserved; no Shell startup file is automatically edited. `DZC_BIN` overrides the embedded CLI path and `DZC_PICKER` chooses `tui`, `fzf`, `dmenu`, or `rofi`. Selection inserts text for review without executing it.

MCP exposes only bounded `clipboard_list`, `clipboard_search`, and `clipboard_get` calls. Sensitive-tagged and encrypted records are excluded before decoding and pagination; the client must still treat returned content as untrusted. Launch the stable executable path with `args: ["mcp"]`; set `DEZIRCLIP_DB_PATH` in the client environment when a specific database is intended. Detailed limits and configuration are in [the CLI manual](../../docs/cli.md).

The older Python adapters remain available for compatibility and Rofi script mode. See [integrations](../../integrations/README.md) only when choosing that route; Python is not needed for native commands.

## Local development

Edit `SKILL.md` and `examples/*.md` directly in the repository. No build step is required for this skill.

To inspect an example before committing:
```bash
cat examples/search-and-add.md
```
Verify the YAML frontmatter is well-formed and the body describes the expected workflow.

When the `skill-creator` tooling is available, run its `scripts/quick_validate.py` against this directory. This checks the skill manifest; validate command behavior separately against the matching CLI.

## Versioning

This skill has **no version field** of its own. It mirrors the in-tree `dzc` CLI version, so there is nothing to bump here. When the CLI version changes, update the skill content and rely on the symlink for propagation.

## Contributing

- Add new examples under `examples/` using the filename pattern `<verb>-<noun>.md` (e.g., `search-and-add.md`).
- Update `SKILL.md`'s patterns section when adding new common workflows so the CLI can surface them.
- Keep examples focused and documented; link optional executable adapters from `integrations/README.md`.

## License

GPL-3.0, matching the project.
