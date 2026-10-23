# Final release identity receipts

This is a Release-only gate between production of the draft assets and
`publish-only`. It does not add PR jobs, new public assets, product metadata
flags, updater keys or publishing permissions. Until the final tagged release
actually runs these jobs, the collector is prepared and package acceptance is
pending. Pure tests use synthetic records and do not prove any release binary.
The current policy accepts only the alpha.2 tag and schema24; later release
versions require a reviewed update of this frozen policy.
This progress connects only the implemented identity gate to `publish-only`.
The independent final NSIS upgrade workflow and its additional publication
dependency are pending a separate progress. Do not create the final release tag
until both reviewed changes and their complete dependencies are integrated.
Installation and upgrade results must come from that workflow's final-byte
reports, never from extraction or the five debug installer scenarios.

The four native jobs download the exact original draft archives. They neither
rebuild products nor substitute the compatible standalone CLI for a desktop
sidecar. Each standalone archive is checked against its checksum, and each of
the ten desktop containers is extracted independently. Every final member has
its own relative path, size and SHA-256; AppImage, Debian, RPM and Flatpak bytes
are not assumed equal after packaging or RPATH changes. AppImage extraction
locates its bounded SquashFS superblock and does not execute the AppImage runtime.
Flatpak import uses a disposable OSTree repository, without installation of the
application, and metadata executes against the matching final Platform runtime.

CLI `build-info --json` and Service `--build-info` output are captured directly.
Desktop metadata comes from the actual WebView's existing `get_build_info`
bridge, using CDP on Windows and WebKitWebDriver on Linux. The collector does
not synthesize BuildInfo or change its target. Every role must report the exact
clean tag source, version, target, schema24, protocol1.0 and linked inventory
(Core/Platform/Protocol/Host, plus Service for that role).

The existing Windows ARM build remains a cross build. Its original bytes are
downloaded to `windows-11-arm`, their PE ARM64 machine is checked, and only
successful native Python, CLI, Service and Desktop execution produces a native
receipt. Failure blocks the gate; it never falls back to invented static JSON.
The runner is listed in GitHub's [official hosted runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
Linux ARM likewise requires the matching actual native runner.

Linux product processes run under separate user/PID/net/IPC/UTS namespaces,
private HOME, DBus, Xvfb and an explicit Secret Service seeded with a fixed
synthetic QA key. The QA helper refuses an existing credential and verifies
its marker, owner, mode and socket boundary. An inherited seccomp filter denies
`add_key`, `request_key` and `keyctl` before any product process starts. Native
safe-invalid probes must see EPERM and an allowed getpid; pure tests also check
the AArch64 and x32 instruction paths. The AArch64 syscall constants follow the
Linux [generic syscall UAPI](https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/unistd.h)
and [audit ABI definitions](https://github.com/torvalds/linux/blob/master/include/uapi/linux/audit.h).
Windows execution is restricted to matching disposable GitHub-hosted runners,
with a new profile beneath RUNNER_TEMP. Downloaded Windows products receive
only an OS/profile environment, not Actions or publishing credentials. Service
metadata does not start a standalone service. Startup of the real Desktop may
initialize its private capture host; there is no native clipboard test command.

Each original report records the actual run URL, job number, source, runner,
asset digest and member digests with actual JSON. Reading this run's job identity
metadata uses only `contents: read` and `actions: read`; it does not fetch CI
diagnostics or logs. Reports are printed with a 256 KiB bound and preserved in
`release-identities-<target>-attempt-<attempt>` Actions artifacts. Attempts retain
separate names so an earlier failure is not overwritten. The aggregate artifact
`release-identities-provenance-attempt-<attempt>` binds all four original report
hashes into the acceptance controller's four standalone and ten container
receipts, without adding anything to the 24 public assets.

Root still reviews the originating reports and compares public bytes during
public acceptance. A report hash is not an independent CI attestation. If the
CodeReview tool cannot expose the final tag run with the required source
binding, retain the run/job/artifact references and report that tool limitation;
do not retrieve CI logs through another API as a workaround. This gate does not
replace the separate real Windows upgrade, two old-version AppImage upgrades,
performance checks, signature verification or later Preview promotion.

Local preparation checks execute no product:

```powershell
python -I -B -m unittest discover -s scripts/tests -p test_release_identities.py -v
node --check scripts/release-identity-desktop.mjs
cargo check --locked -p dzc-platform --example release_receipt_preseed
```
