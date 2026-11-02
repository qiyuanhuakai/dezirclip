# Optional Windows Surface comparison

The `CI` workflow's `windows_surface_performance` manual input defaults to false.
When explicitly enabled, the Windows x64 job builds a release Desktop and native
Surface from one checkout. The existing functional fixture must pass on those
exact bytes before measurement starts. Ordinary push and PR runs never start
the comparison job.

`derive.py` checks reviewed, LF-normalized hashes of the functional controller and
target, then generates temporary plumbing and a target with narrow timing and
resource-observation additions. Generated copies are not maintained as tracked
sources. Functional fixture changes require review and updated base hashes.
The reviewed crash-event reader is copied unchanged, with its own fixed source
hash and a generated-file hash in the derivation receipt.

Each fresh arm uses the same ten ordinary text entries and a 480×360 panel, in
Tauri/MyGo/MyGo/Tauri order. Five warmups precede thirty measured trials. Every
trial sends one Enter and requires the complete selected text in the synthetic
editor, with all original history IDs retained. Each arm also measures visible
and hidden state for 240 seconds after a 30-second settle, nominally every 250 ms.
The main WebView remains constructed and hidden for both renderers.

The clipboard marker written before Desktop starts only calibrates the native
target. Windows capture subscribes to subsequent changes; the fixture does not
require that earlier marker to be imported on startup. After calibration and
Desktop identity checks, it clears history and creates the same ten entries
through the production manual-item command for every arm.

The query-state v4 method uses one additional, unmeasured show before every
trial: native Ctrl+A/Backspace, 700 ms of setup, a bounded observation of
the empty input and at least two distinct visible fixture bodies, then Escape
and actual hidden/target focus confirmation. A blank input with one stale
filtered row is insufficient. Both renderers start each timed show with an
empty search. These extra shows are setup, including the first invocation;
their latency is never pooled into the five warmups or thirty formal samples.
The original 1020-second active and 1080-second total arm budgets still apply.

Before resource sampling, both renderers receive the same query for row 7.
The original-window observer must find that input and exactly one matching
visible fixture body before the unchanged 30-second settle. There are ten
stored rows and one matching visible result. Missing or non-equivalent
data is explicitly unverified and stops the arm; sending input or waiting
700 ms alone is insufficient. These observations are setup, not measurements
of search readiness or first interactive frame.
MyGo uses bounded original-window UIA. The default WebView returned only Pane
nodes in the preserved dd644 run, so Tauri uses a read-only DOM snapshot from
the unique Quick Paste page on the original Desktop's private CDP port, with
native PID/HWND/foreground checks before and after. It checks the top-level
app URL, input value, CSS/viewport-intersecting cards and their DOM text. This
does not establish full-text pixels or a painted frame. No accessibility flag
is forced. Snapshots, expression hash and endpoint identity are preserved.
The two observers may affect caches differently; resource results describe
these controlled observation conditions, not a natural idle footprint.

Latency is a **native visibility and foreground activation proxy**, observed
with a 10 ms polling delay. Actual observation intervals and request overhead
are retained. It is not the first interactive frame and cannot establish the
25% first-interactive improvement gate. The first invocation follows setup and
may compare a prebuilt Tauri window with a newly spawned MyGo helper; it is not
whole-application cold startup. The 700 ms query settle is setup, not a
search-readiness measurement.

Memory is the sum of observed Desktop-family WorkingSet and PrivateCommit;
shared pages may be counted more than once, so these are not PSS or USS. CPU
uses retained original-process counters, including observed processes that
exit, normalized to one logical core. CPU before an identity's first sample and
entirely unseen short-lived processes are omitted. Observer processes are
excluded and can still perturb scheduling. GPU, wakeups, interactive readiness,
IME, DPI and multiple-monitor results remain unmeasured.

The fixture requires a disposable GitHub-hosted Windows runner and explicit
opt-in. It preserves original process handles, native foreground ownership,
the profile/WebView policy transaction, source hashes and cleanup receipts.
Failed arms abort the cohort without retry. Cleanup can terminate owned
processes, so it does not demonstrate natural application shutdown.
Each arm retains the original Desktop state before cleanup and rejects abnormal
exits. Exit code 1 is accepted only after cleanup starts when the original PID
is explicitly recorded as terminated by cleanup. A pre-cleanup abnormal close
may collect bounded, identity-bound Application Error/WER evidence; missing or
unavailable diagnostics never turn that failed arm into a success.

Pure checks, without native execution:

```sh
python -B -m unittest discover -s scripts/tests -p test_windows_performance_derivation.py
python -B scripts/tests/windows-performance/derive.py --output-dir .harness/windows-performance-pure-check
node --test .harness/windows-performance-pure-check/pure.node.mjs
```

Use a fresh output directory for each derivation; existing evidence is never
overwritten. The job retains generated sources/diffs, the build-completion quiet
note, functional receipts, raw per-state resource samples and arm reports. The
quiet note covers sequential work in this job, not the shared physical host.
