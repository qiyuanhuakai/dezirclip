# Windows system-setting ownership and safe upgrades

DezirClip records the exact registry type and raw bytes before it changes
`HKCU\Software\Microsoft\Clipboard\EnableClipboardHistory`,
`EnableCloudClipboard`, or Explorer's `DisabledHotkeys`. A previously missing
value is recorded as missing, rather than replaced by a guessed default.

Receipts live under
`HKCU\Software\io.github.qiyuanhuakai.dezirclip\SystemSettingsOwnership\v1`.
Each receipt is a versioned JSON document stored as `REG_BINARY`, containing
`original` (a raw value or null) and `expected` (the raw value DezirClip wrote).
Only these three fixed destinations can be changed; receipt contents cannot
redirect the restoration to another registry key. Clipboard contents are never
part of this ledger.

Each receipt is persisted before its target value changes. A mutex named
`Global\DezirClip.SystemSettingsOwnership.v1.<current-user-SID>` serializes all
DezirClip processes sharing the user's HKCU, including other Windows sessions.
The SID comes from the current process token; the protected DACL grants access
only to that SID and SYSTEM. Different users receive different names. The
implementation follows Microsoft's
[kernel object namespaces](https://learn.microsoft.com/en-us/windows/win32/termserv/kernel-object-namespaces)
and [CreateMutexW security contract](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createmutexw).
A failure leaves receipts available for
restoration and returns an error. Interrupted batches may retain some overrides
until restoration is retried. The implementation does not depend on Windows
registry transactions, which can be disabled or unavailable. Turning optimization off and uninstalling use the
same restore operation. It restores a value only while its current type and
bytes still match the application's expected value. A later change by the user,
another application, or system policy is preserved. Repeated startup calls also
leave such changes alone until the optimization is turned off; turning it on
again establishes a fresh receipt. Existing settings that already match the
requested setting are never claimed as DezirClip's changes.

`set_windows_clipboard_history(true)` releases only DezirClip's two clipboard
overrides by restoring owned originals; it leaves the Win+V optimization and its
receipt unchanged. It does not force Windows defaults over pre-existing
preferences. Turning history off records owned overrides without
changing unrelated hotkeys. No action deletes the unowned policy values
`DisallowClipboardHistory`, `AllowClipboardHistory`, or
`AllowCrossDeviceClipboard`. If those policies prevent Windows history, the
policy remains authoritative.

## Interactive confirmation and rollback

The GUI uses `begin_registry_win_v_change(enable)`, which returns
`[changed, opaque_token]`. The token refers to a backend-only journal of this
request's successful writes to the three fixed values and their receipts.
Each field is claimed only after its write succeeds. Its original bytes are
read immediately before the write, and its expected bytes are the requested
write value, never a full after-operation snapshot. No raw registry journal is
accepted from the frontend. Pending journals are
limited to 16, and tokens are single use until application exit.

Confirming finishes the token with `finish_registry_win_v_change(token, false)`.
Canceling or failing after begin uses the same command with `rollback: true`.
Rollback restores only values and receipts actually changed by that request,
and only if they still match the recorded expected state. A field that was not
written by this request is never restored, even if an external writer changed
it while begin was still running. For example, an already-unowned V is left
alone when another writer changes DisabledHotkeys to XV during the clipboard
history write; canceling restores this request's clipboard overrides without
removing the external X. An externally changed value on a field written earlier
in begin also remains external because the journal retains the application's
requested value as its expected state. Existing ownership and
newer user changes are preserved. Canceling a disable operation restores the
old active receipts rather than constructing fresh ownership from guessed values.
The toggle is disabled while its confirmation is pending.

A rejected begin does not provide a token; the GUI restores its hotkey and
saved preference without calling the inverse optimization operation. A partial
begin failure attempts its own scoped rollback before returning an error. This
covers both old-value cases: enabling with an unowned V then canceling restores
this request's clipboard overrides; disabling with an unowned V makes no writes
and does not accidentally disable Windows history in the error handler.
If recovery itself fails, write-ahead receipts remain available for retry; the
implementation does not claim filesystem-style atomicity for registry batches.
An old receipt is restored before its target value, whereas a newly created
receipt is removed only after restoring its target value. Each successful
rollback write also advances the token's expected state.
A retry can therefore finish an interrupted value/receipt pair without treating
its own partial progress as an external change; genuinely newer values still remain untouched.

## Installer contract

The hook follows the fixed
[Tauri CLI 2.10.1 NSIS template](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.10.1/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi)
and the planned
[CLI 2.12.3 template](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.3/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi):
`un.onInit` maps `/UPDATE` to `$UpdateMode = 1`, and
`NSIS_HOOK_PREUNINSTALL` executes before the main executable and bundled sidecars
are removed. Update mode preserves all setting receipts and does not run the
restore helper. The official updater's update mode normally skips the old
uninstaller entirely; the guard also covers an uninstaller explicitly invoked
with `/UPDATE`.

For a genuine uninstall, the hook first uses the template's normal running-app
check (a full executable path when Restart Manager macros are available, the
legacy basename for CLI 2.10.1), then waits for `${MAINBINARYNAME}.exe --restore-system-settings`. This
private maintenance entry runs before Tauri, single-instance handling, logging,
database access, clipboard monitoring, or WebView initialization. It accepts no
extra arguments, does not restart Explorer, and exits with 0 on success, 1 on a
restoration error, or 2 on invalid arguments. On restoration failure or a missing
helper, uninstall aborts before deleting application files, so restoration can
be retried. Malformed or unsupported receipts fail closed rather than restoring
unverified data.

The installation location, main binary, bundled `dzc`, and existing PATH handling
are unchanged. Changing a shortcut interactively retains the existing GUI's
explicit Explorer-restart prompt. Uninstall never forces a restart; restored
Explorer hotkeys may take effect at the next Explorer restart or sign-in.

## First migration from older installers

This hook cannot change an uninstaller binary already installed by an older
release. Those binaries may still reset clipboard settings, remove policy values,
and restart Explorer if invoked. A first upgrade should use the official
installer's `/UPDATE` path (for example `setup.exe /P /UPDATE`) or its explicit
option to keep the previous installation and overwrite in place. Do not choose
"uninstall before installing" when migrating from an old uninstaller.
A manual installer that chooses to uninstall is a real uninstall in the Tauri
template, rather than update mode, and runs that installed uninstaller's hooks.

Old modifications have no trustworthy original-value receipt. This release does
not invent one or claim to recover original settings from a previous release.
When an interactive request to turn optimization off would leave an unowned
`DisabledHotkeys` value containing V, it returns an actionable manual-restoration
error. Likewise, enabling Windows history reports an error if the released
clipboard values would still explicitly disable history or cloud clipboard.
The operation checks this before restoring any values, allowing the GUI to keep
its previous hotkey and setting. The maintenance uninstall operation still
preserves all unowned values and succeeds; it does not falsely claim that Windows
history itself was enabled.
Users can review those existing Windows settings and explicitly re-enable
optimization to establish new ownership. Published upgrade claims must be backed
by a real old-installer-to-new-installer test; unit tests cannot prove the
behavior of a previously shipped binary.

## Verification

Inline Rust regressions cover missing originals, exact types/bytes and hotkey
case, no-op ownership, legacy override errors, control-domain isolation, later user changes,
external writes inside begin (including partial begin failure and receipt-only writes), actual isolated HKCU writes,
interrupted-write recovery, unowned policy preservation, and malformed receipts. Registry tests
write only beneath unique `Software\DezirClipInstallerTests` subkeys and clean
them up; they never alter the user's real clipboard or Explorer settings.

`python -m unittest discover -s scripts/tests -p test_installer_hook.py -v`
compiles the real hook with the installed NSIS and runs disposable fixtures.
The fixtures replace Tauri's running-app prompt with a recorder and exercise
both legacy basename and Restart Manager path contracts. They verify genuine
uninstall, `/UPDATE` skipping restoration, helper failure, and missing-helper
abort. They never invoke the real maintenance operation or touch system settings.
Rust tests additionally execute a child process and verify that it waits for
the same user's global mutex until the parent releases it. SID-derived global
names and separation of different SIDs are covered. No actual second Windows
session or different user's access token has been exercised; cross-session
execution and DACL acceptance remain package-level checks.

Controlled execution of the existing GUI handler is recorded during review,
with IPC and dialogs replaced by contract fixtures, without adding a frontend
test framework. Full installed-app upgrades, sidecar replacement, and Windows
ARM64 runtime behavior require acceptance on the corresponding systems.

### Disposable installed-package acceptance

The existing CI workflow also builds unpublished debug NSIS fixtures and runs
four separate, fresh GitHub-hosted Windows jobs. They exercise the digest-pinned
published 0.1.20 uninstaller, old-to-new `/UPDATE`, new-to-new `/UPDATE` with
real ownership receipts, and uninstall after a newer external edit. The owned
scenarios call the actual installed application's begin/finish commands through
a runner-only WebView debugging endpoint, then run its real uninstaller. Typed
Windows values, retained unknown policies, CLI versions, and old history are
checked, with isolated evidence uploaded by the workflow.

These scripts refuse local execution before reading the test registry or
launching an installer. Fixture versions 0.2.0-alpha.0/alpha.1 are never published;
updater artifact signing is disabled only for these debug package-behavior
fixtures, so no production signing secret is used. They do not establish release
build, updater download/signature, AppImage, ARM64, or cross-session acceptance.
The actual CI terminal result is required before claiming these package cases
passed; local syntax checks and the local-execution guard are not installation
results.

The package harness is `scripts/build-native-windows-fixture.ps1`,
`scripts/test-native-windows-upgrade.ps1` and
`scripts/tests/native-windows-ownership.mjs`. The workflow calls the builder
with `-Version 0.2.0-alpha.0` for the owned-upgrade source and
`-Version 0.2.0-alpha.1` for the current candidate. Both installers and their
adjacent `.manifest.json` and generated `.nsi` files remain beneath `RUNNER_TEMP`.
The schema 2 manifest binds the installer, packaged GUI and standalone CLI by
SHA256, with version, target, profile, source commit and lock/configuration/hook
hashes. Updater artifact signing remains disabled in these debug fixtures.

Tauri CLI 2.12.3 changes the GUI's single `__TAURI_BUNDLE_TYPE_VAR_UNK` token to
`__TAURI_BUNDLE_TYPE_VAR_NSS` while building NSIS, then restores the compiled
output. The builder records the original SHA256, token offset and exact expected
packaged SHA256. Acceptance first requires the installed whole-file SHA256 to
match the packaged value, then reverses only that documented token in memory
and requires the original whole-file SHA256. It never rewrites an installed
executable or ignores unrelated byte changes. A missing or duplicate original
token, unexpected offset or either hash mismatch fails acceptance. This behavior
is pinned to the [official bundler implementation](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.3/crates/tauri-bundler/src/bundle.rs).

The same CLI builds all Cargo binaries and NSIS enumerates both the configured
`externalBin` and the non-main Cargo `dzc` binary. Those two sources install to
the same `dzc.exe` destination. Production retains preparation before the GUI
build so configured sidecar inputs exist on a fresh checkout, and repeats it in
the shared `beforeBundleCommand` after the desktop build. The existing build.rs
separately disables tauri-build's sidecar copy to avoid replacing Cargo output
during compilation; that does not change the bundler's two inputs. The shared helper builds `dzc`
with `--locked --no-default-features` and an explicit target triple. It atomically
replaces and verifies the staged external binary and actual Cargo binary inputs
with identical standalone bytes, without writing through Cargo cache hardlinks.
The host build covers both implicit and explicit-host target directories; a
cross build updates its own target directory and leaves host artifacts alone.
Debug/release selection follows `TAURI_ENV_DEBUG`. The portable no-bundle command
explicitly prepares a release CLI with the same `portable` feature as its GUI
before creating the ZIP; it cannot depend on a bundle hook that is not run.
Custom Cargo profiles and manually replacing these hooks are outside these
documented packaging commands.

Native acceptance invokes the real `tauri build --debug --bundles nsis` with
these production hooks intact. It requires a fresh bundle-phase proof binding
the helper, lockfile, exact Cargo arguments and every input path/hash. It also
parses and retains the generated NSIS compiler script, requiring both `dzc.exe`
sources to match that standalone hash and the main executable to match the
proven GUI path. This tests the actual packaging entry point instead of a
fixture-only restaging sequence. The hook order and binary enumeration are
defined by the official [CLI hook](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.3/crates/tauri-cli/src/bundle.rs)
and [NSIS implementation](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.3/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs).

`node --test scripts/tests/packaging-sidecars.node.mjs` uses temporary files and
a fake Cargo runner to cover host/cross/profile/portable paths and cache
hardlinks. `pwsh -NoProfile -File scripts/tests/native-packaging-proof.ps1`
imports only pure byte/path proof functions, without evaluating installer script
bodies, and checks exact token transformations and corrupted inputs. Its optional
`-EvidenceZip` argument can additionally verify the retained original failed CI
GUI bytes; it only reads them and writes `.bin` evidence. Neither local check is
a native installation result. The original failed run remains failed; the fixed
package requires a new successful disposable-runner acceptance run.

The acceptance script rejects an existing registered installation, a running
GUI/CLI, ownership receipts or pre-existing application data directories. It
creates two new `datapath.txt` files beneath the real RoamingAppData identifiers,
encoded without a BOM, pointing to one synthetic `RUNNER_TEMP` profile. It does
not pretend that changing `APPDATA` changes Tauri's known-folder API. The CLI
override points at that profile's `clipboard.db`; the actual installed GUI must
report the same directory through `get_data_path`. Installed GUI and CLI hashes,
the CLI's built-in version and the GUI's actual `plugin:app|version` result must
match the candidate manifest. This app/version permission is already present
in the main window's capability. Old history must both round-trip through the
GUI command and render in the actual WebView.

Before the WebView checks, acceptance verifies the launched process's actual
image path. It refreshes the process handle and reads `Path` from a fresh
`Get-Process` query, then rechecks the original handle for exit so PID reuse
cannot establish identity. Only an empty or temporarily unreadable path is
retried, for at most 2 seconds and 41 observations with 50 ms polling. A nonempty
different path or an exited process fails immediately; a correct path also
requires a reliable alive state. The launch command and `StartInfo` are never
used as evidence of the observed image. Each `*-process-identity.json` retains
the expected path, every observed path, elapsed time, alive/exit state and query
errors, and `*-process.json` includes the same identity result even on failure.
The whole-file GUI/CLI and NSIS token proofs remain mandatory before launch.
`scripts/tests/native-process-identity.ps1` exercises injected observations
without starting or inspecting a real process. A subsequent successful
disposable-runner job is required to establish actual upgrade acceptance.

For the three current-candidate scenarios, a runner-only CDP connection observes
the real listener while a PowerShell STA subprocess executes `Set-Clipboard`
with synthetic A, B and A again. Text travels as environment data, never shell
source. Each capture must produce an ID-only `clipboard-captured` payload;
recaptured A must retain its single stored ID, and the legacy full-entry event
must not be used for those captures. The installed CLI then queries the same
stored IDs. This bridge neither mocks invoke nor emits simulated capture events.
The real uninstaller must remove both the GUI executable and the bundled CLI.

Explorer is compared as a complete, sorted PID/start-time set, including an
empty set. Current-candidate cases forbid both disappearance and new instances.
Hosted runners may have no Explorer: such evidence reports
`existingExplorerRestartVerified=false` and `noExplorerSpawned=true`; it does
not prove behavior with an active Explorer desktop. The legacy negative control
records its process changes without claiming safe restoration.

All three scripts reject local or self-hosted execution before installer,
clipboard or registry operations. Candidate/input/output paths reject escapes
from `RUNNER_TEMP` and reparse points. The actual package script additionally
refuses existing GUI data instead of overwriting it. Started fixture processes
are stopped by their process handles and their process trees; the next GUI run
also requires the CDP port to be released. Cleanup removes only a redirect still
containing the harness's own profile path, and removes its directory only if
empty. Changed redirects and other files remain untouched. No recursive data
cleanup is performed. Test registry values and clipboard contents belong to the
disposable hosted VM and are left to runner disposal rather than an unrelated
cleanup routine that could hide uninstall failures.

`dezirclip-upgrade-*/acceptance.json` records success/failure and installer
process results, including failures before a GUI result is available. The
workflow's existing `always()` artifact step retains that directory, copied
candidate/previous manifests, typed registry snapshots, process sets, synthetic
history, GUI/CLI results, stdout/stderr and WebView screenshots when available.
The original installer binaries are not automatically uploaded by that step.
Local parser checks, guard rejection and pure path/manifest contract checks are
the only permitted local validation; all actual package/capture claims await
the integrated GitHub-hosted jobs at their final source revision.
