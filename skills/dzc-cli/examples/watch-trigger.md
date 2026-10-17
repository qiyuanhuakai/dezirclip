# Example: Watch for sensitive patterns

## Scenario

You are working with production credentials or customer PII and want an automatic
guardrail. The `dzc watch` subcommand streams new clipboard entries as they
arrive. You can pipe that stream into a matcher and trigger a notification
whenever a sensitive pattern is detected.

> **`--pattern` is a case-insensitive substring match, not a regular expression.**
> `a|b` matches the literal text `a|b` and never matches `password` or `secret`.
> Run one watcher per term, or filter the stream with `grep -Ei` after the fact.

> **Why step 2's pattern is a bare substring and not a quoted key.**
> `dzc watch --json` emits NDJSON whose keys are fixed — `id`, `content_type`,
> `content`, `preview`, `tags`, `pinned`, `timestamp`. A secret almost never
> appears under one of those names; it appears *inside* `content`, as a string
> value. A pattern like `"(password|secret|token)"` therefore matches only when
> the clipboard content is itself a JSON document carrying those quoted keys —
> on ordinary entries such as `password=example` or `export API_KEY=...` it
> reports nothing at all.

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
  | grep -Ei --line-buffered '(password|passwd|secret|api[_-]?key|access[_-]?key|token)\b' \
  | while read -r line; do
      PREVIEW=$(echo "$line" | jq -r '.preview // "no preview"')
      notify-send "Sensitive clipboard detected" "$PREVIEW"
    done
```

   `--line-buffered` is about `grep`, not about `dzc`. `dzc` already writes one
   line per event and flushes it, so every match reaches `grep` promptly — but
   GNU `grep` block-buffers *its own* stdout whenever it is not writing to a
   terminal, so without the flag a single match sits in `grep`'s buffer until
   4 KB accumulate or the stream ends. Since a sentinel typically sees one
   match now and then, that means the notification can arrive minutes late, or
   never arrive while the watcher is still running. Measured on GNU grep 3.11:
   same input, stdout to a pipe, upstream left open — default buffering
   delivered 0 bytes after 700 ms and only flushed at EOF, `--line-buffered`
   delivered the line within the same 700 ms.

   `-i` because `API_KEY=` is as common as `api_key=`, and the trailing `\b` so
   `tokenizer` or `passwordless` do not drag every match in. There is
   deliberately **no** left-hand `\b`: `db_password` and
   `AWS_SECRET_ACCESS_KEY` put the keyword in the middle of a longer identifier,
   where a leading boundary would not fire.

   Two limits worth knowing. A credential that carries no keyword at all —
   a bare `ghp_abc123` — matches nothing here, exactly as it would miss
   `--pattern password`. And prose that merely mentions a keyword
   ("my db password is in the vault") does match; for a sentinel that is the
   cheaper error, but tune the alternation if it is too loud.

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
- No `--unbuffered` is needed on the `dzc` side. Each entry is emitted with
  `println!`, and Rust's `Stdout` is a `LineWriter` — line-buffered whether
  stdout is a terminal or a pipe — so every event reaches `grep` on its own
  newline. That covers `dzc`'s own output and nothing else: every stage further
  down the pipe has its own buffer, which is why step 2 passes
  `--line-buffered` to `grep`.
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
