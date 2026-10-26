# Phase 6 private Surface control protocol

Status: implementation contract for the experimental Quick Paste helper. The
published alpha5 release is unchanged. Tauri remains the default renderer.

## Transport and trust

Desktop creates exactly one managed helper with private inherited stdin/stdout.
Each frame is a four-byte big-endian unsigned byte length followed by UTF-8
JSON. The limit is 64 KiB, excluding the prefix. Empty, oversized, malformed,
unknown-type and unexpected-direction frames fail the session. Stdout contains
only frames; diagnostics use bounded stderr and exclude grants and content.
Requests and acknowledgements have a five-second deadline. Shutdown waits for
the actual child exit; a failed stop prevents updating its executable.

Every envelope is `{ "protocol_version": {"major":1,"minor":0},
"request_id":"…", "type":"…", "payload":{…} }`. IDs are nonempty bounded
ASCII identifiers (at most 128 bytes). A reply echoes the original request ID.
One writer serializes each stream; responses may arrive after cancellation and
must then be discarded. Neither side retries a paste automatically.

`RuntimeState` is the existing Wire type, with its exact field names and safe
integer bounds: `service_instance_id`, `privacy_epoch`, `history_revision`,
`event_sequence`. Do not introduce a second epoch or revision representation.
Within one Host instance, `history_revision` is a monotonic observation of both
the persistent database revision and the versioned RAM queue. Every mutable RAM
guard advances its version while holding that queue's lock, including edits,
reordering and rollback. Surface pagination holds a read guard across both state
observations and the query; it does not depend on a later capture notification.
Unchanged reads do not advance the version. Counters outside the safe integer
range fail closed. Core's database-only `RuntimeSnapshot` remains a separate
type and is never compared as a Surface/Host state token.

## Initialization and version identity

Desktop sends `initialize` before any show. Its payload is:

```text
surface_session_id       opaque fresh ID for this actual child lifetime
endpoint                 current authenticated local Host endpoint
grant                    finite role=surface, permissions=[history_read]
state                    RuntimeState from that actual Host
build                    {component_version, source_revision, build_id, target}
locale                   bounded locale identifier
theme                    {mode, opacity}
```

Mode is `light` or `dark` and opacity is a finite number in [0,1]. The helper
uses the generated shared design tokens for that mode. No arbitrary CSS, scripts, resource paths,
database paths, control credentials, OS window handles or clipboard bodies are
passed. The helper compares `build` with its compiled identity. Unknown source,
different version/build set or incompatible protocol fails initialization.

The helper connects with `client_role=surface`, its real version, this grant,
and `required_capabilities=["surface.history.page"]`. It requires the version-1
Surface page capability and compatible actual Host build identity, and compares
the handshake RuntimeState before reading. Only after successful initialization
and IPC negotiation does it reply `ready` with the same `surface_session_id`, its
compiled `build`, and current `state`. A failed initialization replies `failed`
with a stable error code and exposes no body or secret.

## Display and actions

All display messages include `surface_session_id` and `display_request_id`.
Desktop generates a new display ID for every activation; hiding, switching
renderer/Host or cancelling invalidates it. The helper never chooses the paste
target. Desktop keeps that target in an internal request-bound ticket.

| Direction | Type | Additional payload |
|---|---|---|
| Desktop → helper | `show` | `state`, `rect:{x,y,width,height}`, `scale`, `locale`, `theme`, optional generic `notice` |
| helper → Desktop | `shown` | actual `state`; acknowledgement after the current panel is presented |
| Desktop → helper | `hide` | `reason` (`cancel`, `paste`, `host_changed`, `privacy_changed`, `disabled`, `update`) |
| helper → Desktop | `hidden` | acknowledgement only for that display and hide request |
| helper → Desktop | `paste` | `action_id`, `entry_id`, selected `state` |
| Desktop → helper | `paste-result` | `action_id`, `performed`, stable `error_code` or null |
| helper → Desktop | `dismiss` | current display ID, `reason` (`cancel`, `blur`), no selected entry/body |
| Desktop → helper | `shutdown` | `reason` (`disabled`, `update`, `exit`, `host_changed`) |
| helper → Desktop | `stopped` | same session; Desktop still confirms actual process exit |

`x/y` are signed 32-bit physical desktop coordinates (negative monitor origins
are valid); width/height are positive bounded physical extents; scale is finite
and positive. Desktop computes geometry and checks the current monitor/DPI.
The helper does not return a native handle or request arbitrary activation.

A paste is admitted once for the current session/display/action ID and the
original selected RuntimeState. Desktop checks the actual Host instance, epoch
and history revision; stale selections fail and require a fresh query/selection.
It requests hide and waits for the matching `hidden`, checks its own actual
target/focus, and performs the existing Core-authorized selected-copy and paste
flow with `SensitiveAccess::Denied`. The exact production clipboard/focus order
and checks remain owned by Desktop. A late hide ACK, action, old Host response
or cancelled display never resumes an old paste. An uncertain paste result is
not a reason to repeat the keystroke. Success does not authorize deleting an
entry or consuming an unrelated queue item.

An explicit Esc/close cancellation can restore only that display's original
native target, while its generation remains current and the foreground is
still the managed helper/owned Desktop or the original target. A blur dismiss
never restores focus. Cancellation remains effective while the parent awaits
a hidden ACK or Host response. A delayed restore worker checks the request
generation again before native activation; new shows/Host changes invalidate it.

The parent-only `desktop.surface.copy` extension carries `entry_id` and the
original selected `RuntimeState`. It requires the private Desktop grant,
rechecks instance/epoch/revision around Core `SensitiveAccess::Denied` selected
copy, and performs no paste, queue consumption, use-count update or deletion.
The helper is never granted this method. Desktop rechecks the same state after
copy and immediately before native paste, and checks the retained target again.

A failed action retains only `paste_unavailable`, without its entry/body/action.
The next explicit Surface show can carry this notice as a persistent warning
independent of query success. Alternatively the next painted, visible main
window receives the translated toast. Hidden-window emission alone does not
consume the pending notice. Neither path reopens an old display or repeats input.

Before granting access or starting the UI, Desktop runs the fixed sibling's
bounded `--build-info` path and requires the same verified build set, MyGo
v0.6.4/source 6255ea40139d06e0762a7c4a32626148ba410478 and toolkit patch
`mygo-v0.6.4-no-startup-cleanup-v1`. This branch returns before App.Run. An old
unpatched/unknown helper is rejected before the upstream startup cleanup can run.

## Read-only Surface IPC

`surface.history.page` returns bounded summaries and pagination with one
`RuntimeState`; it never returns body, HTML, attachments, image data or keys.
The request contains `service_instance_id`, `expected_epoch`,
`expected_history_revision`, `limit`, `offset`, optional `query`/`mode` and
ordinary metadata filters. The Host checks these before and after the Core
query and rejects a changed snapshot. A nonempty query uses the existing Core
search semantics. The first implementation limits pages to 100 summaries.

The helper assigns each query an increasing local `query_seq`. It displays a
result only while its session, display, query sequence and returned state still
match. A newer query cancels the old result locally; cancelled IPC responses are
still drained/correlated rather than mistaken for the next response. A revision
change refreshes state and restarts pagination, never silently retargets a
pending action. Host/epoch changes, a broken transport or `resync_required`
discard old data and renegotiate as required by the existing events protocol.

The helper receives no copy/paste/surface-control, Desktop-history, privacy-write
or service-management grant. Its only interactive actions go through its private
parent channel. Embedded and Service modes use the same actual capture owner;
an unavailable Service does not authorize reading a local database instead.
Desktop revokes the finite grant on child exit, renderer/Host switch or update.
Only Host owns global shortcuts, native target focus and clipboard capture.

## Lifecycle acceptance boundary

Test private framing, direction/correlation, source/version checks, finite
read-only grants, stale positive/negative IDs, epoch/revision changes, cancelled
searches, late hide ACKs and duplicate actions before native UI acceptance.
Native Windows/X11 acceptance separately covers actual focus restoration,
repeated hide/show, IME, DPI, multiple monitors and helper exit during update.
No test may turn these future checks into existing alpha5 release evidence.
