# Example: Search and tag potential secrets

## Scenario

You pasted several passwords, API keys, and tokens while debugging a staging environment.
You want to find those entries and mark them as `sensitive` so they never leak into
shared exports or logs.

## Steps

1. List recent text entries and grab their IDs.

```bash
tiez-c list --kind text --ids
```

2. Search for secret-like patterns using regex and return only IDs.

```bash
tiez-c search "password|api[_-]?key|secret|token|auth" --mode regex --ids
```

3. Pipe the matching IDs into the tag command to add the `sensitive` label.

```bash
tiez-c search "password|api[_-]?key|secret|token|auth" --mode regex --ids \
  | xargs -I {} tiez-c tag add {} sensitive
```

4. Confirm the tag was applied by listing entries with the new tag.

```bash
tiez-c list --tag sensitive --json | jq '.[].preview'
```

5. (Optional) Verify the total count of newly tagged items.

```bash
tiez-c list --tag sensitive --json | jq 'length'
```

## Expected output

`tiez-c search ... --ids` prints one ID per line, e.g.:

```
c3a1b2
f4e5d6
a7b8c9
```

After tagging, `tiez-c list --tag sensitive --json | jq '.[].preview'` returns
an array of preview strings whose content matched the original regex.

`jq 'length'` shows the number of entries now carrying the `sensitive` tag.

## Why this matters

Clipboard history is a common source of credential leakage. Tagging secrets
immediately keeps them out of bulk exports and lets you filter them with a
single `--tag sensitive` flag.

## Variations

- Narrow the regex to a specific service: `"github_pat|slack_token"`.
- Tag with multiple labels at once by running the command repeatedly or
  extending the script to call `tiez-c tag add` twice per ID.
- Exclude already-tagged entries by adding `--not-tag sensitive` to the
  `search` call if your CLI supports it.
