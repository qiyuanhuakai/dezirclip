# tiez-c-cli skill — Developer Notes

## Layout

```
skills/tiez-c-cli/
├── SKILL.md              # Skill manifest (commands, parameters, examples)
├── install.sh            # Linux/macOS installer (symlink)
├── install.ps1           # Windows installer (junction)
├── README.md             # This file
└── examples/
    └── search-and-add.md # Example workflow: search + add pattern
```

## Installing for end users

**Linux / macOS:**
```bash
bash skills/tiez-c-cli/install.sh
```

**Windows (PowerShell):**
```powershell
powershell -ExecutionPolicy Bypass -File skills/tiez-c-cli/install.ps1
```

**Uninstall:**
```bash
bash skills/tiez-c-cli/install.sh --uninstall
```
```powershell
powershell -ExecutionPolicy Bypass -File skills/tiez-c-cli/install.ps1 --uninstall
```

## Updating the skill

This skill uses a **symlink/junction model** for installation.

- **Symlink model (default):** The installer creates a symlink (or junction on Windows) from `~/.claude/skills/tiez-c-cli` back to the source directory in the repository. To update, pull the latest changes in the repo — no re-install needed because the symlink always points to the current tree.
- **Copy model:** If you instead copy the skill directory manually into `~/.claude/skills/`, re-run the installer after pulling updates to refresh the copy.

## Local development

Edit `SKILL.md` and `examples/*.md` directly in the repository. No build step is required for this skill.

To sanity-check an example before committing:
```bash
cat examples/search-and-add.md
```
Verify the YAML frontmatter is well-formed and the body describes the expected workflow.

## Versioning

This skill has **no version field** of its own. It mirrors the in-tree `tiez-c` CLI version, so there is nothing to bump here. When the CLI version changes, update the skill content and rely on the symlink for propagation.

## Contributing

- Add new examples under `examples/` using the filename pattern `<verb>-<noun>.md` (e.g., `search-and-add.md`).
- Update `SKILL.md`'s patterns section when adding new common workflows so the CLI can surface them.
- Keep all examples ≥ 30 lines and avoid executable scripts inside examples — they are documentation-only.

## License

MIT or Apache-2.0, matching the project.
