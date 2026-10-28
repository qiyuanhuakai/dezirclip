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
