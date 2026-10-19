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
`[changed, opaque_token]`. The token refers to a backend-only snapshot of the
three fixed values and their receipts before and after this particular request.
No raw registry snapshot is accepted from the frontend. Pending snapshots are
limited to 16, and tokens are single use until application exit.

Confirming finishes the token with `finish_registry_win_v_change(token, false)`.
Canceling or failing after begin uses the same command with `rollback: true`.
Rollback restores only values and receipts actually changed by that request,
and only if they still match the recorded after-state. Existing ownership and
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
Each successful rollback write also advances the token's expected after-state.
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
case, no-op ownership, legacy override errors, control-domain isolation, later user changes, actual isolated HKCU writes,
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
