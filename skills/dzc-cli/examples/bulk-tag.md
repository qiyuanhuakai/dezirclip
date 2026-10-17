# Example: Bulk-tag today's URLs

## Scenario

Throughout the day you copy dozens of links: documentation pages, pull requests,
and vendor dashboards. At the end of the day you want every URL entry to carry
the `web` tag so you can filter them later with `dzc list --tag web`.

Two things to keep in mind: entry IDs are integers, and `timestamp` is epoch
**milliseconds**, so a seconds-based epoch needs a `* 1000`. The positional
limit caps `list` at 20 rows by default — pass `-- -1` to scan everything.

## Steps

1. Calculate the Unix epoch **in milliseconds** for the start of today
   (midnight local time).

```bash
TODAY_START=$(( $(date -d 'today 00:00' +%s) * 1000 ))
echo $TODAY_START
```

2. List all entries in JSON, filter to those created today whose content
   contains `http://` or `https://`, then extract their IDs.

```bash
dzc list --json -- -1 \
  | jq -r ".[] \
    | select(.timestamp >= $TODAY_START) \
    | select(.content | test(\"https?://\")) \
    | .id"
```

3. Pipe the IDs into the tag command to add the `web` label to each one.

```bash
dzc list --json -- -1 \
  | jq -r ".[] \
    | select(.timestamp >= $TODAY_START) \
    | select(.content | test(\"https?://\")) \
    | .id" \
  | xargs -I {} dzc tag add {} web
```

4. Verify that the tag was applied by counting entries with `web`.

```bash
dzc list --tag web --json | jq 'length'
```

5. Spot-check the first few tagged entries.

```bash
dzc list --tag web --json | jq '.[0:3] | .[].preview'
```

## Expected output

The `jq` filter in step 2 emits one numeric ID per line, for example:

```
142
377
904
...
```

`dzc tag add` then prints `tag: added "web" to id=142` for each entry.

After tagging, `jq 'length'` returns a non-zero integer equal to the number of
URL entries captured today.

The spot-check in step 5 prints previews of the first three tagged items so you
can confirm the filter caught actual links and not random text.

## Variations

- Tag yesterday's URLs by shifting the epoch: `TODAY_START=$(( $(date -d 'yesterday 00:00' +%s) * 1000 ))`.
- Narrow to a specific domain:

```bash
dzc list --json -- -1 \
  | jq -r ".[] \
    | select(.timestamp >= $TODAY_START) \
    | select(.content | test(\"https?://github.com\")) \
    | .id" \
  | xargs -I {} dzc tag add {} github
```

- Tag all images from today by switching the regex to `test("\\.(png|jpg|jpeg|gif|webp)")`,
  or use `dzc search "..." --mode regex --json` when you already know the pattern.
- Drop the `web` label again with
  `dzc list --tag web --ids | xargs -I {} dzc tag remove {} web`.
