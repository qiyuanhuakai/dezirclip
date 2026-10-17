# Example: Watch for sensitive patterns

## Scenario

You are working with production credentials or customer PII and want an automatic
guardrail. The `dzc watch` subcommand streams new clipboard entries as they
arrive. You can pipe that stream into a matcher and trigger a notification
whenever a sensitive pattern is detected.

> **`--pattern` is a case-insensitive substring match, not a regular expression.**
> `a|b` matches the literal text `a|b` and never matches `password` or `secret`.
> Run one watcher per term, or filter the stream with `grep -E` after the fact.

## Steps

1. Start a watcher that filters entries containing a single secret-like
   substring and prints the full JSON payload.

```bash
dzc watch --pattern "password" --json
```

2. Cover several terms at once by filtering the unfiltered stream. This is the
   portable way to do alternation, since `dzc` itself only does substrings.

```bash
dzc watch --json \
  | grep -E '"(password|secret|api[_-]?key|token)"' \
  | while read -r line; do
      PREVIEW=$(echo "$line" | jq -r '.preview // "no preview"')
      notify-send "Sensitive clipboard detected" "$PREVIEW"
    done
```

3. Or run several dedicated watchers, one per term, in parallel.

```bash
dzc watch --pattern "password" --json >> ~/.dzc-sentinel-password.log &
dzc watch --pattern "api_key"   --json >> ~/.dzc-sentinel-apikey.log &
```

4. Append every match to a log file for later audit.

```bash
dzc watch --pattern "password" --json >> ~/.dzc-sentinel.log
```

5. (Optional) Inspect the growing log file without interfering with the watcher.

```bash
tail -f ~/.dzc-sentinel.log | jq .
```

## Expected behavior

- `dzc watch --pattern "..." --json` blocks and streams one JSON object per
  matching clipboard event, polling every 500 ms by default
  (`--interval`, clamped to `[50, 60000]`).
- The pattern is matched against the entry's `content`, falling back to
  `preview`.
- The shell loop in step 2 calls `notify-send`, which displays a transient
  desktop bubble on most Linux desktop environments.
- The log file in step 4 grows monotonically. Each line is valid JSON, so you
  can replay or filter the file later with `jq`.
- The startup banner goes to stderr; entries go to stdout, so the stream stays
  pipeable. `--quiet` suppresses the banner only.
- Stopping the watcher (Ctrl-C) ends the stream with exit code `130`; the log
  file remains intact.

## Notification methods

`watch` has no notification flag — pipe it into whatever notifier you use.

- Desktop pop-up: `notify-send` on Linux with a notification daemon running.
- Log file: append `--json` output to a dated file such as
  `~/.dzc-sentinel-$(date +%Y%m%d).log`.
- Webhook: pipe the stream into `curl` if you want to forward matches to an
  alerting endpoint. dezirclip itself makes no network calls.

## Cleanup

- Rotate logs with `logrotate` or a simple cron job so `~/.dzc-sentinel.log`
  does not grow without bound.
- Stop the watcher process when you are done auditing; it polls the database
  in a long-lived loop.
