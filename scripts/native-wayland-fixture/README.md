# Private Wayland protocol tests

These tools exercise synthetic CLIPBOARD data inside a private headless compositor. They never attach to the caller's display, session bus, HOME or database. They are functional checks, not performance measurements or a claim of full native desktop support.

The default invocation only prints preparation metadata. Execution requires an explicit `--execute`, Linux with bubblewrap/user namespaces, and a new `/tmp/dezirclip-*` output directory. Do not run the Rust native tests directly against a user's graphical session. All reports and failed attempts remain in their output directories.

## Build the negative protocol fixture

Install development tools providing `cc`, `pkg-config`, `wayland-scanner`, `wayland-server` and the official staging ext-data-control XML. No Cargo development dependency or clipboard command line utility is used.

```sh
python3 scripts/build-wayland-protocol-fixture.py \
  --output /tmp/dezirclip-protocol-fixture-unique
# Inspect the preparation metadata, then use the same arguments with --execute.
```

The builder records the input C/XML hashes, actual compiler/scanner/server versions, command arguments, logs and output hash in `build.json`. The generated service advertises deliberately missing interfaces or immediately finishes the data device. It does not own a selection or return clipboard data. These cases test client failure handling, not compositor permission UI.

## Build native test inputs

Use a separate target and at most two build jobs. Clear inherited graphical environment variables so ordinary unit tests cannot discover WSLg or a user's desktop.

```sh
env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u DBUS_SESSION_BUS_ADDRESS \
  XDG_SESSION_TYPE=x11 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/dezirclip-wayland-target \
  cargo test -p dzc-platform --lib --locked --message-format=json > /tmp/dezirclip-platform.jsonl
env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u DBUS_SESSION_BUS_ADDRESS \
  XDG_SESSION_TYPE=x11 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/dezirclip-wayland-target \
  cargo build -p dezirclip --no-default-features --bin dzc --locked --message-format=json > /tmp/dezirclip-cli.jsonl
```

Extract the `compiler-artifact.executable` for target names `dzc_platform` and `dzc` from these JSON streams; do not guess a cached target path. Supply the resulting absolute files:

```sh
python3 scripts/test-native-wayland.py \
  --test-binary /absolute/compiler-artifact-platform-test \
  --cli /absolute/compiler-artifact-dzc \
  --compositor /usr/bin/sway --swaymsg /usr/bin/swaymsg \
  --protocol-fixture /tmp/dezirclip-protocol-fixture-unique/protocol-fixture \
  --expected-protocol wlr --output /tmp/dezirclip-wayland-matrix-unique
# Inspect the preparation metadata, then use the same arguments with --execute.
```

The controller starts its own compositor, waits for an actual Sway IPC response, verifies the negotiated protocol, runs the native tests and standalone CLI MIME round trips, then terminates its own compositor to check disconnect cleanup. It also runs the generated negative server. Inputs, runtime files and the controller are hashed before and after. A failed test or changed input fails the matrix; no automatic retry selects a later successful sample.

## ext and wlr coverage

The alpha.2 functional matrix used Ubuntu24.04.4 WSL, system Sway1.9/wlroots0.17.1 for wlr v2 and a separately installed Sway1.11/wlroots0.19.0 for ext v1. For the latter, use `--expected-protocol ext`, the private `sway`/`swaymsg`, and `--library-prefix /tmp/dezirclip-private-compositor`. The prefix must contain `lib/` and a file named `dezirclip-isolated-compositor-prefix` containing exactly `private synthetic clipboard only` followed by a newline. Its files and symlinks must remain within the prefix. This marker is an isolation guard, not an authentication mechanism. Only that prefix is mounted read-only at `/test-libs`; the test receives its `LD_LIBRARY_PATH` inside the sandbox.

The private headless build used these official release inputs, installed only into its temporary prefix:

| Input | SHA256 |
| --- | --- |
| [Wayland1.23.1](https://gitlab.freedesktop.org/wayland/wayland/-/releases/1.23.1) | `864fb2a8399e2d0ec39d56e9d9b753c093775beadc6022ce81f441929a81e5ed` |
| [Pixman0.44.2](https://cairographics.org/releases/pixman-0.44.2.tar.gz) | `6349061ce1a338ab6952b92194d1b0377472244208d47ff25bef86fc71973466` |
| [libinput1.29.2](https://gitlab.freedesktop.org/libinput/libinput/-/tags/1.29.2) | `02631a861fc6b9fb9ca6ae51cf863334e0699bf60c91c389ac420f3fa0053bdc` |
| [wlroots0.19.0](https://gitlab.freedesktop.org/wlroots/wlroots/-/tags/0.19.0) | `967f3112c82e8ea18cbdc513e22196b30ccc9fad3fb836f1cff80312c66fab96` |
| [Sway1.11](https://github.com/swaywm/sway/releases/tag/1.11) | `034ec4519326d6af5275814700dde46e852c5174614109affe4c86b2fbee062a` |

Build dependencies in that order with Meson, `--prefix` pointing to the private prefix, `--libdir lib` and `meson compile -j2`. Tests/docs/examples were disabled. wlroots was configured with `-Dxwayland=disabled -Dbackends=[] -Drenderers=[] -Dallocators=[] -Dsession=disabled -Dcolor-management=disabled -Dlibliftoff=disabled -Dxcb-errors=disabled`; its always-built headless/pixman path supplies this test. Sway used `-Dswaybar=false -Dswaynag=false -Dtray=disabled -Dgdk-pixbuf=disabled -Dman-pages=disabled -Ddefault-wallpaper=false`. The fixture's actual version output and negotiated protocol, rather than installed package names alone, establish coverage.

Windows native subscription tests and separate private Xvfb XFixes/polling checks cover the other platform paths. Wayland source identity, PRIMARY, global shortcuts, auto-paste and portal grants remain unsupported; the test does not route them through XWayland.
