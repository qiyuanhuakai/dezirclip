# Example: Bulk-tag today's URLs

## Scenario

Throughout the day you copy dozens of links: documentation pages, pull requests,
and vendor dashboards. At the end of the day you want every URL entry to carry
the `web` tag so you can filter them later with `dzc list --tag web`.

## Steps

1. Calculate the Unix epoch for the start of today (midnight local time).

```bash
TODAY_START=$(date -d 'today 00:00' +%s)
echo $TODAY_START
```

2. List all entries in JSON, filter to those created today whose content starts
   with `http` or `https`, then extract their IDs.

```bash
dzc list --json \
  | jq -r ".[] \
    | select(.timestamp >= $TODAY_START) \
    | select(.content | test(\"https?://\")) \
    | .id"
```

3. Pipe the IDs into the tag command to add the `web` label to each one.

```bash
dzc list --json \
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

The `jq` filter in step 2 emits one ID per line, for example:

```
b1c2d3
e4f5a6
...
```

After tagging, `jq 'length'` returns a non-zero integer equal to the number of
URL entries captured today.

The spot-check in step 5 prints previews of the first three tagged items so you
can confirm the filter caught actual links and not random text.

## Variations

- Tag yesterday's URLs by shifting the epoch: `date -d 'yesterday 00:00' +%s`.
- Narrow to a specific domain:

```bash
dzc list --json \
  | jq -r ".[] \
    | select(.timestamp >= $TODAY_START) \
    | select(.content | test(\"https?://github.com\")) \
    | .id" \
  | xargs -I {} dzc tag add {} github
```

- Tag all images from today by switching the regex to `test("\\.(png|jpg|jpeg|gif|webp)")`.
