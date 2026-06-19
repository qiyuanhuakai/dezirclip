# Example: Watch for sensitive patterns

## Scenario

You are working with production credentials or customer PII and want an automatic
guardrail. The `tiez-c watch` subcommand streams new clipboard entries as they
arrive. You can pipe that stream into a matcher and trigger a notification
whenever a sensitive pattern is detected.

## Steps

1. Start a watcher that filters entries matching secret-like patterns and prints
   the full JSON payload.

```bash
tiez-c watch --pattern "password|secret|api[_-]?key|token|auth" --json
```

2. Send a desktop notification on Linux whenever a match appears. The watcher
   prints one JSON object per line; `jq -r` extracts the preview for the alert.

```bash
tiez-c watch --pattern "password|secret|api[_-]?key" \
  | while read -r line; do
      PREVIEW=$(echo "$line" | jq -r '.preview // "no preview"')
      notify-send "Sensitive clipboard detected" "$PREVIEW"
    done
```

3. Append every match to a log file for later audit.

```bash
tiez-c watch --pattern "password|secret|api[_-]?key" --json \
  >> ~/.tiez-sentinel.log
```

4. (Optional) Inspect the growing log file without interfering with the watcher.

```bash
tail -f ~/.tiez-sentinel.log | jq .
```

## Expected behavior

- `tiez-c watch --pattern "..." --json` blocks and streams one JSON object per
  matching clipboard event.
- When a match occurs, the shell loop in step 2 calls `notify-send`, which
  displays a transient desktop bubble on most Linux desktop environments.
- The log file in step 3 grows monotonically. Each line is valid JSON, so you
  can replay or filter the file later with `jq`.
- Stopping the watcher (Ctrl-C) ends the stream; the log file remains intact.

## Notification methods

- Desktop pop-up: `notify-send` on Linux with a notification daemon running.
- Log file: append `--json` output to a dated file such as
  `~/.tiez-sentinel-$(date +%Y%m%d).log`.
- Webhook: pipe the stream into `curl` if you want to forward matches to a
  Slack channel or internal alerting endpoint.

## Cleanup

- Rotate logs with `logrotate` or a simple cron job so `~/.tiez-sentinel.log`
  does not grow without bound.
- Stop the watcher process when you are done auditing; it holds a long-lived
  connection to the clipboard backend.
