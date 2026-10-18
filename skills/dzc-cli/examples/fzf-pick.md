# Example: FZF interactive picker

The optional adapter handles cancellation and native copying directly:

```bash
python3 integrations/dzc-integrations.py pick --backend fzf --action copy
```

It requires Python 3.10+, fzf and the matching dzc version. Windows uses
`python`. See `integrations/README.md` for the TUI, dmenu/rofi and shell
editing-buffer widgets. These adapters live in the source checkout and are
separate from the bundled skill.

## Scenario

You have dozens of clipboard entries and you want to pick exactly one to inspect
or reuse. Typing an ID by hand is tedious, so you use `fzf` to browse the list
interactively and feed the selection back into `dzc get`.

## Prerequisites

- `fzf` installed and available on `PATH`.
- `jq` installed for the JSON variant.
- A terminal that supports 256-color output for best UX.

## Steps

1. List entry IDs and launch `fzf` for selection. `list` returns the 20 most
   recent rows by default; add `-- -1` to browse everything.

```bash
dzc list --ids | fzf
dzc list --ids -- -1 | fzf
```

2. Pass the selected ID to `dzc get` to print the entry's content.

```bash
dzc list --ids | fzf | xargs dzc get
```

3. (Optional) Browse with the preview alongside each row, then print the
   chosen entry.

```bash
dzc list --tsv -- -1 \
  | fzf --with-nth=2 --delimiter='\t' \
  | cut -f1 \
  | xargs dzc get
```

4. (Optional) Restrict the picker to entries of one content type. `--kind`
   filters by content type; `--tag` filters by tag.

```bash
dzc list --kind image --ids | fzf --prompt "image> " | xargs dzc get
```

## Expected behavior

- `fzf` opens a searchable list. Typing narrows the candidates in real time.
- Pressing Enter returns one ID to stdout.
- `xargs` feeds that single ID into `dzc get`, which prints the entry's content
  followed by a newline. Add `--json` for the full record (type, tags, pin
  state, timestamp) and `--preview` for the short preview instead.
- The variant in step 3 shows one `id + preview` row per candidate, so you can
  match on the preview text without leaving the picker.

## Customization

- Change the prompt with `--prompt "clipboard> "`.
- Filter the candidate list first: `dzc list --kind text --ids | fzf`.
- Use `fzf --multi` to select several IDs and then loop over them:

```bash
dzc list --ids | fzf --multi | tr '\n' '\0' | xargs -0 -n1 dzc get
```

- Show each candidate's type and tags instead of the preview:
  `jq -r '.[] | "\(.id)\t\(.type)\t\(.tags | join(","))"'`.
- Pipe the result straight into another command — `get` writes content only, so
  `dzc list --ids | fzf | xargs -I {} dzc get {} > picked.txt` is safe.

Use `dzc get ID --raw` when an added newline would change the selected text.
Use `dzc copy ID` to restore images/files as native clipboard content without
xclip/xsel/wl-copy. Printing a record does not insert it into a parent shell's
edit line; use the optional shell widgets for that purpose.
