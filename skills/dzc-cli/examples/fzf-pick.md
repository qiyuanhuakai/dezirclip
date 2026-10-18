# Example: Native terminal and fzf picker

These native commands are available since `v0.1.20-alpha.2`. Check `dzc --help`
before using them with older releases. They do not need Python, jq, or a running
DezirClip GUI.

## Select and copy

The default picker is the built-in TUI; external fzf is optional:

```bash
dzc pick
dzc pick --backend fzf
dzc pick --backend dmenu
dzc pick --backend rofi
```

An external backend must be installed on PATH. Rofi uses its dmenu interface.
The default action copies the selected record to the native system clipboard,
preserving text, HTML, images, or file-list representations. Native clipboard
access requires an accessible Windows/X11 desktop session.

## Capture selected content

```bash
dzc tui --query project --kind text --action print > picked.txt
dzc pick --backend fzf --action print > picked.txt
dzc pick --backend fzf --action id
```

`print` emits the complete selected text without adding a newline; `id` emits
one numeric ID and a newline. The TUI renders on the controlling terminal so it
does not contaminate captured stdout. Esc, q, or Ctrl-C cancels the TUI with a
nonzero exit status and no selected content. Shell redirection creates the
output file before selection, so cancellation may leave an empty file.

The picker lists at most 100 entries by default; `--limit` accepts 1–200 and
`--offset` skips matching entries. TUI `/` searches, PageUp/PageDown changes
pages, `p` toggles pin, `t/u` adds/removes a tag, and `d` requests confirmed
deletion. `Enter` performs the chosen action. Local interactive views may show
sensitive records; use the read-only MCP or `--exclude-sensitive` history
queries for filtered Agent access.

## Insert into a Shell editing buffer

Printing a selection does not populate a parent Shell's command line. Load the
native widget for the current Shell, then press Ctrl-G:

```bash
# Bash 5+
eval "$(dzc bash)"
# Zsh
eval "$(dzc zsh)"
# Fish
dzc fish | source
```

PowerShell 7 with PSReadLine uses `dzc pwsh | Out-String | Invoke-Expression`.
Nushell 0.116+ uses
`dzc nu --install ($nu.user-autoload-dirs.0 | path join 'dzc.nu')`, then restart
Nushell. The widget inserts selected text without executing or submitting it.
`DZC_PICKER=fzf` chooses fzf instead of the built-in TUI; `DZC_BIN` can point
at a stable full executable path.

## SSH and legacy builds

In an SSH session with a PTY, explicitly use `dzc tui --action osc52` or
`dzc pick --backend fzf --action osc52` to send text to the local terminal's
clipboard. This requires terminal permission and is limited to 100000 original
UTF-8 bytes. Use `--osc52-passthrough tmux` only when one layer of tmux wrapping
is needed. OSC52 does not read the local clipboard or synchronize history.

For older CLI builds, the optional matching Python adapter remains available:

```bash
python3 /absolute/path/to/integrations/dzc-integrations.py pick --backend fzf --action copy
```

That compatibility route requires Python 3.10+ and the complete runnable
integration directory, not just the launcher file. Windows commonly uses
`python`. See [integration setup](../../../integrations/README.md) and
[the CLI manual](../../../docs/cli.md) for version, path, and OSC52 details.
