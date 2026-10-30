# Optional Windows Surface comparison

The `CI` workflow's `windows_surface_performance` manual input defaults to false.
When explicitly enabled, the Windows x64 job builds a release Desktop and native
Surface from one checkout. The existing functional fixture must pass on those
exact bytes before measurement starts. Ordinary push and PR runs never start
the comparison job.

The separate `windows_surface_crash_diagnostics` dispatch input runs the original
thirteen release functional checks in up to five predeclared, sequential fresh-profile
cases with no comparison. The two optional modes
are mutually exclusive. Diagnostic-only dispatches can collect the target crash
after ordinary checks fail; those failures remain failed. Performance still
requires successful frontend and backend prerequisites. This hosted-runner-only
collector uses a new private
directory and an exclusively created per-application
[WER LocalDumps configuration](https://learn.microsoft.com/en-us/windows/win32/wer/collecting-user-mode-dumps).
It preserves existing registry settings, restores only its unchanged owned
child key in `finally` and an always cleanup step, and leaves an empty parent
key if it had to create one. Forced runner loss cannot guarantee cleanup.

Only a dump bound to the retained original PID, process creation second, exact
module path, candidate hash and time window is eligible for upload. Parsing
reads bounded metadata; raw full dumps are capped at 1 GiB and retained for
three days. The original functional failure remains a failure. A successful
run without a crash is `not_reproduced`, and does not close historical heap
failures. The scheduler advances only after both original exits are zero, the original
exit verdict is allowed, fixed build/source hashes and original input integrity agree,
WER cleanup is confirmed restored, and collection says `not_reproduced` with no
candidate or accepted dump. Every failure, exception or invalid receipt stops the
sequence immediately; failed cases are never retried or replaced. Five qualified
successes are only `not_reproduced_5`, and do not establish a fix.

The private sequence manifest records all five planned cases, exact workflow identity,
fixed inputs, each started case's outputs and exit, qualification or stop reason,
and metadata hashes. Before another case starts it preserves each original WER root's
top-level JSON and functional root's top-level JSON/log/PNG, including empty logs.
Copies are bounded to 16 MiB per file, 128 MiB and 256 files per case; overflow stops
the sequence. No profile, candidate directory or raw dump is traversed or copied.
The original global workflow outputs remain attached to the child, so always-cleanup
targets the only active WER transaction. Metadata upload includes the complete private
sequence directory and the last original roots; an error before the first child can
still retain the parent manifest. The existing separate dump upload accepts only the
original collector's identity-bound path, and an accepted dump always stops dispatch.

The external-blur input uses the same original alternate editor and at most three
predeclared client points: left inset 8, right inset 8, then center, all at mid-height.
Duplicate points are removed. Read-only conversion and hit probes record every point,
including points outside the virtual desktop, and select the first exact original
editor hit. Immediately before the single original three-event mouse batch, the
fixture rechecks the original process/window/thread/class/parent identities, foreground,
geometry and selected hit. Any change stops without selecting another point or sending
input. It does not activate, move, raise or hide a window, move the pointer during
planning, wait for coverage to clear or repeat a click. Hit metadata contains class,
PID, parent/root/owner and geometry, with no third-party title, content or image reads.
These read-only probes cannot make the final input atomic with window changes.

This is new-cohort input planning. The `08eabd190f9ef10fa04350a6f076745f89f62e6c`
diagnostic run `41048645997` remains failed at its second case's original fixed-point
precheck; its third through fifth cases remain unstarted. The original controller's
thirteen functional assertions, timing and post-click focus/blur checks remain intact.
The input change does not reproduce or fix the historical heap failure.

Release optimization and stripping are unchanged; debugger tools are
inventoried without installation, so this first collector does not promise a
symbolic stack or identify the corruption origin.

`derive.py` checks reviewed, LF-normalized hashes of the functional controller,
target and lifecycle helper, then generates temporary plumbing and a target with narrow timing and
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

The 364bfd2 performance request (run 41075863232) failed its debug functional
prerequisite before any release ABBA arm started. Native observation repeatedly
reported an unavailable original process handle while product status reported
the helper visible. The report retained the original show timeout; failed
cleanup then left the Desktop and its pipes alive until the operator cancelled
the job. This does not establish that MyGo never displayed, or a heap-crash cause.
Its original report, cleanup error and cancellation are preserved together.

The subsequent fixture adds bounded failure cleanup only. Discovery errors stay
fatal and prevent WebView-policy restoration, while cleanup still attempts only
already-retained original handles. Failed handle acquisition records PID, exact
requested access, phase and Windows error; failed memory queries remain null.
The follow-up ownership fix separates opened handles from authenticated owned
handles. Only a complete exact-image Desktop bind or a fully read descendant
tree rooted in that authenticated Desktop authorizes termination of an exact
retained handle and identity. Rejected or incomplete binds remain failed,
including a failed open that left no handle. Unvalidated opened handles are
closed without termination and keep ownership unknown, so WebView-policy
restoration is blocked. Previously authenticated objects can still be cleaned
after a later discovery error. Ordinary aa511b CI passed its normal path; that
result does not exercise or close its rejected-bind P2.
New descendant authorization also confirms every parent edge in a second process
snapshot taken after all candidate handles are retained, and requires each
child's creation time to be no earlier than its retained parent's. This rejects
stale ParentPID links to a newer reused PID and a PID replaced between the
first snapshot and its first open. Every candidate retains the same exact
handle and identity across confirmation. Every new or pending candidate
requires a complete unchanged edge before any new candidate is promoted.
The original 922 fixture's fake older-child
counterexample is preserved separately. This is fixture ownership validation,
with no claim of a Windows crash cause or change to the Linux producer.
The stale ParentPID case follows [Microsoft's documented PID-reuse example](https://devblogs.microsoft.com/oldnewthing/20150403-00/?p=44313).

The a11cb83 request (run 41106374215) passed twelve release functional checks
before the thirteenth check's initial main-window observation rejected PID 7624.
That same authenticated original WebView2 handle, creation time and image had
already been observed exited with code 0. The original error combines changed
and disappeared parent-snapshot entries; the two snapshots were not serialized.
The supervisor preserved exit 1 and confirmed its original controller's exit.
Native cleanup receipts confirmed all sixteen known objects exited; no ABBA arm started.
The intermediate 527c128 terminal-state change retained an authenticated object's
terminal observation and counters only while its exact handle and identity
still matched. Slot checks preceded that exception. New, pending and still-live
candidates retained both snapshots and every creation-time edge check. There was
no new observation retry, input, wait or resumed failed case. The original
failed run remains failed, and the earlier heap-corruption risk remains open.

The 527c128 request (run 41115292407) was stopped by the operator after a
source review and all-fake counterexample found an independent snapshot error
path. This method stop does not establish an actual Windows API error, product
failure or heap crash in that run. Snapshot enumeration now accepts a false
First/Next result only with `ERROR_NO_MORE_FILES` (18); any other error records
its first/next phase and Windows error and aborts discovery. A partial process
map cannot authorize new handles or certify complete cleanup. The snapshot
handle still closes in `finally`, with no enumeration or input retry. See
Microsoft's [Process32First](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-process32first)
and [Process32NextW](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-process32nextw)
return-value contracts.

The a7676e2 request (run 41120519234) failed its own debug prerequisite after
twelve checks passed. In check thirteen, raw responses 137 and 141 satisfy the
unchanged main-destruction predicate, and 141 satisfies the actual cold native
panel/foreground predicate. The unchanged controller then requests Escape;
response 142 rejects PID 5392 in the key command's initial observation, before
Escape input. Earlier 141 recorded that original WebView2 object live; final
cleanup recorded the same identity exited with code 0, without forced termination.
The error combines changed/disappeared parent entries and does not serialize
either snapshot or the failed request's certificate state. This does not prove
the exact race or a product/heap cause. A separate readonly UIA timeout at
response 38 remained in the original log; its search/selection check later passed.
The complete thirteenth check remains failed, and no release ABBA result exists.

Current ownership follows the lifetime of a fully authenticated original object,
independently of its current live/terminal state. Each query freezes the prior
handle/identity certificates, verifies their slots before and after fresh
process reads and after the second snapshot, and fails on any changed slot or
query exception even when an original object is absent from process snapshots.
Discovery starts from these retained, generation-fixed objects, including an
exited parent whose children can remain alive. Every new or pending descendant
still requires both parent snapshots and each creation-time edge; only the
complete validated batch gains certificates. Existing certificates are not
rewritten. Only certified observations enter the returned process/window scope;
unvalidated pending handles stay close-only and keep cleanup failed. Failure-only
parent diagnostics retain bounded PID/parent/state fields. There is no new
input, observation retry, wait, permission, deadline or performance-sampling
method. See Microsoft's [process handle lifetime](https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers),
[process termination](https://learn.microsoft.com/en-us/windows/win32/procthread/terminating-a-process),
and [process-object/PID lifetime explanation](https://devblogs.microsoft.com/oldnewthing/20110107-00/?p=11803).

Node records process exit separately from stdio close. A failed stop request
still enters a bounded close wait; fallback termination targets only its original
spawned child. A kill request, destroyed pipe or detached event handle never
counts as confirmed application cleanup. An unconfirmed CDP close is a cleanup
failure. After synchronously preserving the failed report and raw child logs,
the CLI flushes its output for at most one second and exits nonzero even if an
unconfirmed socket remains active.

Each functional controller also has a separate 600 second Python supervisor,
with at most five seconds to confirm its original child after termination is
requested. Its exclusive metadata receipt preserves an original nonzero code;
a total timeout returns 124 and does not certify product cleanup. Ordinary CI
limits this step to 12 minutes. The success-only five-case diagnostic step is
limited to 55 minutes so its collector can retain WER restoration in finally.
The release functional prerequisite uses the same supervisor and its own
12 minute step. The following complete ABBA step retains its original arm
budgets and 150 minute job budget. These bounds
do not change the thirteen functional assertions or allow retries. Any future
validation is a new complete source cohort; no old case or failed arm is resumed.

Pure checks, without native execution:

```sh
python -B -m unittest discover -s scripts/tests -p test_windows_performance_derivation.py
python -B -m unittest discover -s scripts/tests -p test_windows_crash_sequence.py
python -B scripts/tests/windows-performance/derive.py --output-dir .harness/windows-performance-pure-check
node --test .harness/windows-performance-pure-check/pure.node.mjs
```

Use a fresh output directory for each derivation; existing evidence is never
overwritten. The job retains generated sources/diffs, the build-completion quiet
note, functional receipts, raw per-state resource samples and arm reports. The
quiet note covers sequential work in this job, not the shared physical host.
