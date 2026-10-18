# Example: Search and tag potential secrets

## Scenario

You pasted several passwords, API keys, and tokens while debugging a staging environment.
You want to find those entries and mark them as `sensitive` so filtered queries
can exclude them. Tags alone do not remove content from backups.

Entry IDs are integers. `search --ids` prints one ID per line without content.

## Steps

1. List recent text entries and grab their IDs.

```bash
dzc list --kind text --ids
```

2. Search for secret-like patterns using regex and extract the IDs.

```bash
dzc search "password|api[_-]?key|secret|token|auth" --mode regex --ids --limit=-1
```

3. Pipe the matching IDs into the tag command to add the `sensitive` label.

```bash
dzc search "password|api[_-]?key|secret|token|auth" --mode regex --ids --limit=-1 \
  | xargs -I {} dzc tag add {} sensitive
```

4. Confirm the tag was applied by listing entries with the new tag.

```bash
dzc list --tag sensitive --ids -- -1
```

5. (Optional) Verify the total count of newly tagged items.

```bash
dzc list --tag sensitive --metadata --json -- -1 | jq 'length'
```

## Expected output

Step 2 prints one numeric ID per line, e.g.:

```
142
377
904
```

`dzc tag add` prints `tag: added "sensitive" to id=142` per entry.

After tagging, `dzc list --tag sensitive --ids -- -1` returns the tagged IDs
without printing clipboard content.

`jq 'length'` shows the number of entries now carrying the `sensitive` tag.

## Why this matters

Use `--exclude-sensitive` on list, search or get to exclude tagged and encrypted
records before decoding. `--tag sensitive` selects those records instead.
Exports include sensitive records; protect backup files and use encrypted export
when appropriate. A tag does not enforce access control or redact existing logs.

## Variations

- Narrow the regex to a specific service: `"github_pat|slack_token"`.
- Tag with multiple labels at once by running the command repeatedly or
  extending the script to call `dzc tag add` twice per ID.
- Restrict the result set before pagination with `dzc search "..." --kind text --mode regex --json`.
- Exclude already-tagged entries by filtering in `jq`, e.g.
  `jq -r '.[] | select((.tags | index("sensitive")) == null) | .id'`.
