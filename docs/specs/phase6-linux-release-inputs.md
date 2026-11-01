# Linux release input supply

The existing CI workflow has an optional `linux_release_inputs` dispatch input,
defaulting to false. It adds work only to the hosted Linux x64 `tauri` job after
its original checks. Windows, NSIS, release settings, caching and the existing
checks retain their behavior. It does not repeat the frontend or Surface build.
The existing Frontend CI job also runs the focused standard-library fake contract
tests, including when this dispatch input is false.

The producer reuses the original frontend and native Surface build receipt. It
checks their bytes, clean source identity, target, toolkit patch and recorded
Surface runtime identity. It runs one Desktop command:

```text
cargo build --release --locked --manifest-path src-tauri/Cargo.toml --features tauri/custom-protocol --bin dezirclip --message-format=json
```

Cargo's compiler-artifact and successful build-finished messages must identify
the original release binary and profile. The producer does not run the Desktop
or add another Surface execution. Its Desktop proof explicitly identifies the
GHA producer and Cargo metadata verification; it is not a namespace offline
builder receipt or runtime build-info verification. The existing Surface proof
is copied without alteration.

The artifact contains the two original ELF byte sequences, their proofs,
frontend hashes, actual toolchain output, raw Cargo stdout/stderr, build metadata
and checksums. Cargo's two raw logs share a 32 MiB limit; a quota, I/O, parse,
command or identity error preserves failure and cannot produce a complete pair.
The new build has a 3,000-second pipe deadline, a 30-second final wait and a
60-minute CI step limit. Error logs can be incomplete exact prefixes; failures
are never certified as a complete input pair. This 32 MiB build-metadata bound
does not change the separate Native performance log policy.
Existing output directories are refused. The producer exclusively writes,
flushes and reads back `inputs.pending.json` after all payloads and metadata have
been verified, then atomically renames it to `inputs.json` as its last operation.
The pending file is never a completion marker. Consumers must also require the
corresponding producer step and job to have succeeded, verify the original
hashes, and reject an artifact containing `failure.json`.
`SHA256SUMS` covers payloads, proofs and metadata; the final manifest separately
binds that checksum file. Artifact downloads may require restoring executable
mode; file hashes bind bytes, not downloaded Unix permissions.

These are product inputs only. No historical functional, idle, quiet-window,
one-shot or performance qualification belongs to this candidate. Those fields
remain null and `phase6_pass` remains false. Supplying the pair does not start a
cohort or establish Phase 6 acceptance.
