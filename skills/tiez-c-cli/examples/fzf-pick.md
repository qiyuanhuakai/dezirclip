# Example: FZF interactive picker

## Scenario

You have dozens of clipboard entries and you want to pick exactly one to inspect
or reuse. Typing an ID by hand is tedious, so you use `fzf` to browse the list
interactively and feed the selection back into `tiez-c get`.

## Prerequisites

- `fzf` installed and available on `PATH`.
- `jq` installed for the preview variant.
- A terminal that supports 256-color output for best UX.

## Steps

1. List all entry IDs and launch `fzf` for selection.

```bash
tiez-c list --ids | fzf
```

2. Pass the selected ID to `tiez-c get` to see the full entry.

```bash
tiez-c list --ids | fzf | xargs tiez-c get
```

3. (Optional) Show a preview window with the first 80 characters of each entry
   while you browse.

```bash
tiez-c list --json \
  | jq -r '.[] | "\(.id)\t\(.preview)"' \
  | fzf --with-nth=2 \
  | cut -f1 \
  | xargs tiez-c get
```

4. (Optional) Restrict the picker to entries with a specific tag.

```bash
tiez-c list --tag image --ids | fzf --prompt "image> " | xargs tiez-c get
```

## Expected behavior

- `fzf` opens a searchable list. Typing narrows the candidates in real time.
- Pressing Enter returns one ID to stdout.
- `xargs` feeds that single ID into `tiez-c get`, which prints the full metadata
  and content of the chosen entry.
- The preview variant keeps a two-pane view: the left pane shows the picker,
  the right pane updates with the formatted entry preview as you move the cursor.

## Customization

- Change the prompt with `--prompt "clipboard> "`.
- Filter the candidate list first: `tiez-c list --kind text --ids | fzf`.
- Use `fzf --multi` to select several IDs and then loop over them:

```bash
tiez-c list --ids | fzf --multi | tr '\n' '\0' | xargs -0 -n1 tiez-c get
```

- Replace `jq -r '.[] | "\(.id)\t\(.preview)"'` with a shorter format if your
  CLI supports `--preview`-style output natively.
