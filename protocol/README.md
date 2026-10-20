# Local protocol v1

`schema/*.schema.json` is the wire contract. `scripts/generate-protocol.py`
generates Rust/Go DTOs and their offline schema bundle. Run it with `--check`
to detect drift. Shared accepted/rejected messages live in
`fixtures/messages.json`; both language implementations validate these against
the same schema before decoding DTOs. Existing CLI JSON and MCP JSON-RPC remain
separate interfaces.

Each frame is a four-byte unsigned big-endian byte count followed by one UTF-8
JSON value, at most 1 MiB. Zero lengths, incomplete frames, invalid UTF-8/JSON,
unknown methods and invalid parameters fail. Transport operations have bounded
deadlines. Requests/responses use matching nonempty request IDs. Optional
unknown fields are forward compatible; event data has a closed metadata shape
and cannot contain clipboard bodies. There is no multipart event payload.

The first request is `hello`. Major versions must match; minor versions and
optional capabilities are negotiated. An unknown required capability fails.
`client_role` is descriptive: it does not grant any permission. The Host maps
OS-authenticated current-user sessions and its own explicit grants to Core
authorization. Tokens are never emitted in errors or events. Summary messages
are distinct from trusted embedded database access.

Linux uses a 0700 current-user directory, a 0600 Unix-domain socket, a native
endpoint ownership lock and `SO_PEERCRED`. Windows uses a local-only named pipe
with a protected current-user DACL, first-instance protection, client token SID
and session validation after reading hello. Clients verify the server's user
identity too. Windows Go explicitly requests identification impersonation,
rather than the dependency's anonymous default. These controls do not promise
isolation from a malicious process already executing as the same OS user.
Linux peer UID/PID come from the kernel. Its display name only separates local
endpoint namespaces and is not evidence of a peer's graphical-session identity.
The Windows negative user/session comparisons are unit controls; they do not
claim an actual second Windows account/session was exercised locally.

The reusable Go client has no UI dependency. It uses a bounded synchronous call
at a time, validates responses and closes on transport/correlation failures.
Reconnection requires a new hello and an authoritative state refresh; event
delivery is not durable. A new Host instance resets its sequence under a new
instance ID, while Core owns durable history revision and privacy epoch.

Native transport fixture (synthetic data, no clipboard/GUI/database):

```text
cargo build -p dzc-platform --features ipc --example ipc_protocol_fixture -j2
python scripts/test-local-ipc.py --rust-fixture target/debug/examples/ipc_protocol_fixture --go /path/to/go
```

On Windows the Rust fixture path has `.exe`. `clients/go` uses the standard Go
module tooling, independently of the pnpm-managed frontend. Production code
does not fetch schemas or open TCP sockets.
