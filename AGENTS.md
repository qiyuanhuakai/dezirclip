# PROJECT KNOWLEDGE BASE

**Updated:** 2026-11-27

**Architecture baseline:** `1f1641a0455bfcdbc72b4ba70a24d3cb810887a9`

**Implementation base:** `v0.2.0`

**Implementation plan:** [DZC-ARCH-001](docs/specs/architecture-simplification.md)

**Current progress:** [Architecture implementation](docs/architecture-implementation.md)

## OVERVIEW

DezirClip is a local clipboard workspace for Windows and Linux. Its Rust Core, Platform, Protocol, Host and Service crates support Tauri + React, MyGo, CLI/TUI, MCP and shell clients. Tauri defaults to Embedded Host; MyGo uses the Rust Service. Ordinary local CLI commands may use Core directly; explicitly requested Host modes must not silently fall back to database access.

The signed updater is the permitted networking exception. Future macOS/Swift work follows the separate M0–M4 gates in DZC-ARCH-001; current releases remain Windows/Linux. The design is an evolution of the existing code, not a second Core or a requirement to route all clients through IPC.

## STRUCTURE

| Location | Current responsibility |
|---|---|
| `crates/dzc-core` | Business APIs, SQLite history, settings, privacy, attachments, backups and injected ports |
| `crates/dzc-platform` | OS clipboard/input, permissions, local IPC, process identity and key providers |
| `crates/dzc-protocol` / `protocol/schema` | Shared wire contracts; schema author source and generated Rust/Go representations |
| `crates/dzc-host` | Embedded/Service coordination, grants, sessions, capture ownership, events and snapshots |
| `crates/dzc-service` | Service implementation and the `dzc-service` executable |
| `crates/dzc-cli` | CLI, TUI, MCP and shell integrations, including the `dzc` executable |
| `src-tauri` | Tauri commands, window/tray/hotkey wiring, desktop lifecycle and transitional legacy adapters |
| `src` | React product features, hooks, desktop adapters, locales and visual styles |
| `clients/go` / `frontends/dzc-surface` | Host client SDK and MyGo Native desktop. React compatibility code has a separate Web build |
| Versioned Deziro UI dependencies | Shared React/Native components and generated presets. DezirClip integrates them under `src` and `frontends/dzc-surface`. Retain license boundaries |
| `scripts` / `integrations` | Build, package, release, native acceptance, performance and compatibility tools |
| `.github/workflows` | Six current CI/release workflows; actual triggers and evidence are documented in `docs/ci.md` |
| `docs/specs` | Current behavior contracts and the staged architecture cleanup specification |

Generated files, inline tests and historical evidence are counted separately from handwritten product behavior. Do not use total physical lines as a deletion target. `clients/swift` contains the pre-SwiftUI handshake and transport package and the signed Darwin helper acceptance client.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Add Tauri command | `src-tauri/src/app/commands/*.rs` + register in `main.rs` invoke_handler | Pattern: split by domain (clipboard, settings, hotkey, file, ui, system, history) |
| Add React UI | `src/features/<feature>/components/` + `hooks/` if stateful | Each feature is self-contained; re-export via barrel only if needed |
| Add Rust platform code | `crates/dzc-platform/src`; inspect existing desktop-specific adapters when migrating | Platform must not depend on Tauri app code or Core repositories/SQL |
| Modify theme | `design/themes.json` for metadata and the versioned Deziro UI packages for built-in CSS and presets | The settings menu order, label keys and Windows backdrop mapping come from the catalog |
| Add i18n key | Edit `src/locales/zh.ts` first, then `en.ts`/`tw.ts`, then run `pnpm run i18n:check` | `zh.ts` is the canonical source |
| Configure window/security | `src-tauri/tauri.conf.json` + `src-tauri/capabilities/default.json` | Window starts `visible: false, focusable: false, transparent: true` |
| Test pure Rust logic | Inline `#[cfg(test)] mod tests` in the owning crate | Keep std-only assertions and existing fixtures; native/CLI integration also has separate scripts |

## CONVENTIONS (project-specific deviations)

### Multi-window via URL routing (NOT separate HTML entries)
`src/main.tsx` checks `?window=compact-preview` and renders `<CompactPreviewWindow />` vs `<App />` in a single bundle. Adding a new window = add URL param branch, not a new HTML file.

### Theme preloading before React hydration
`main.tsx` reads `localStorage.tiez_theme` synchronously, then `import.meta.glob("./styles/themes/*.css")` lazy-loads the chosen CSS **before** React renders. Avoids flash of default theme. **Never inline theme CSS** — keep it in `src/styles/themes/`.

### Feature-sliced frontend, no global state library
Each feature owns its hooks. No Redux/Zustand/Jotai. Cross-feature state flows via props or custom events emitted from Tauri (`app.emit(...)` ↔ `listen(...)` on the frontend).

### Rust dependency and ownership boundaries

- Core depends on Protocol and existing foundations; it accepts injected ports/providers and does not import concrete Platform or UI implementations.
- Platform may use controlled public Core APIs, ports and providers, but not repository types, history connections or SQL.
- Host depends on Core/Platform/Protocol and owns authenticated sessions, capture coordination and event contracts.
- Tauri keeps window lifecycle and native UI coordination and uses Core's public APIs. The current Core has no legacy feature. The capture benchmark reads archived source layouts when measuring historical baselines.
- Tauri manages the `HistoryService` opened once with the desktop initialization policy. Desktop commands and Embedded Host share its repositories and caches.
- Do not replace the GUI initializer with `HistoryService::open` without preserving the existing PRAGMA, migrations, seed and shared session policy.
- `global_state.rs` contains cross-thread lifecycle atomics. Do not replace hot-path CAS coordination with a global Mutex.

### Repository hygiene

Generated build output and session artifacts remain ignored. The tracked `AGENTS.md` is the curated project knowledge base. Preserve independent lockfiles, generated mirrors and historical evidence when they have real consumers; retire compatibility code only after its replacement or end of support is explicit.

### Manual Vite chunk splitting
`vite.config.ts` splits output by feature (`settings`, `tag`, `emoji`, `compact-preview`) and by vendor (`vendor-react-select`, `vendor-motion`, `vendor-virtuoso`, `vendor-tauri`, `vendor-react`). When adding a heavy new feature, add a `manualChunks` entry to keep cold-start parse time bounded.

### Linux service opt-out via env var
`DEZIRCLIP_DISABLE_LINUX_SERVICES=window_tracker,edge_docking,mouse_hotkey` is used by `pnpm run tauri:dev:safe`; the existing `TIEZ_DISABLE_LINUX_SERVICES` alias remains accepted. Keep supported service opt-outs when moving startup wiring.

## ANTI-PATTERNS (THIS PROJECT)

1. **NO `TODO` / `FIXME` / `HACK` / `WORKAROUND` / `XXX` comments.** Project enforces zero tech-debt markers. The only `DO NOT` comment in the entire codebase is at `src/features/settings/components/groups/DataSettingsGroup.tsx:42` (explains deferred DB copy). Any new such marker = immediate review rejection.

2. **NO `as any` / `@ts-ignore` / `@ts-expect-error` / `// @ts-nocheck`.** `tsconfig.json` runs `strict + noUnusedLocals + noUnusedParameters`. The Rust side has zero `unsafe` outside of clearly-bounded Win32/Linux syscall blocks (see `src-tauri/src/infrastructure/`).

3. **NO committing `dist/` or `src-tauri/target/`.** Despite these appearing in the working tree, they are gitignored. Build output must NEVER be committed.

4. **Current releases remain Windows/Linux; macOS/Swift preparation is allowed under DZC-ARCH-001.** Preserve existing Windows/Linux behavior while separating Linux-specific assumptions. Actual Darwin code, Swift SDK/UI and release claims require the separate M0–M4 platform work and real acceptance. Do not create empty platform implementations or report unavailable capabilities as successful.

5. **NO networking features beyond the explicitly approved signed updater.** Cloud sync, MQTT and AI assistant remain removed. The v0.2 updater may use its fixed GitHub release endpoints for signed application updates; local IPC Host/Service does not permit remote network access. See `docs/updater.md` and `docs/specs/v0.2-execution.md` for this limited exception.

6. **NO inline theme CSS.** All styles must live under `src/styles/`. No `<style>` blocks in components.

7. **NO cargo `[dev-dependencies]` section.** All tests use inline `#[cfg(test)] mod tests` with std-only assertions. No `mockall`, no `rstest`, no `criterion`.

8. **NO frontend test framework EXCEPT scoped vitest exemption for NEW pure-logic hooks/utils only.** 已有代码、UI 组件、UI hooks 禁止添加测试。AGENTS.md 修订日期 2026-06-17，附 `roadmap-2026 G16 guardrail。

9. **NO ESLint / Prettier / rustfmt config.** Project relies on `tsc --strict` + `cargo clippy` (not configured either) + code review. Adding these is a deliberate decision, not an oversight.

10. **NO new general-purpose CI workflows.** Work within the existing CI/release entry points; the two historical identities/upgrade workflows remain until current consumers and supported upgrades are accounted for. Follow `docs/ci.md` for actual execution and DZC-ARCH-001 C0–C7 for proposed changes. Do not describe source-acceptance, candidate sealing or recovery targets as already implemented. Normal local frontend builds use pnpm scripts.

11. **NO npm commands for project management.** `package.json` pins `packageManager: pnpm@11.0.9`; use `pnpm install`, `pnpm run ...`, and `pnpm exec ...` for all frontend/Tauri package tasks. Do not use `npm install`, `npm run ...`, or `npx` in project instructions or local verification commands.

## UNIQUE STYLES

### Tailwind-free, hand-rolled CSS architecture
- `src/styles/tokens/` — CSS custom properties (`global.tokens.css`, `theme.tokens.css`, `mode.tokens.css`)
- `src/styles/themes/` — full theme variants (`retro`, `mica`, `acrylic`, `macos`, `scifi`, `liquid-glass`)
- `src/styles/theme-core/` — cross-theme shared rules (`*.shell.shared.css`, `*.dialog.shared.css`)
- `src/styles/components/` — per-component CSS (`clipboard.css`, `settings.css`, `tags.css`, etc.)
- Theme switch is runtime-only; no build-time theme stripping

### WebView2 memory hint + idle-destroy as the memory-pressure strategy
Preserve the existing combination of memory hints, idle destruction and the explicitly configured GPU option:
1. `app/webview_memory.rs` calls `ICoreWebView2_19::SetMemoryUsageTargetLevel(LOW)` via Tauri `with_webview` whenever the window hides
2. `app/idle_destroyer.rs` runs a background thread that destroys the webview after N seconds of inactivity (default 60s, range 5–3600)
3. `app/gpu_switcher.rs` optionally collapses the WebView2 GPU process entirely via `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (user-toggleable via `app.disable_webview_gpu` setting, requires webview recreate to take effect)

The four-atomics state machine in `global_state.rs` (LAST_HIDDEN_TIMESTAMP, WINDOW_LIFECYCLE, IS_DESTROYED, RECREATE_PENDING) coordinates the destroy/recreate cycle. **Never read/modify these without understanding the CAS ordering rules** — see `src-tauri/src/app/idle_destroyer.rs`.

### Platform-conditional Rust

Shared native operations belong in `crates/dzc-platform`. Existing Tauri modules also contain desktop-specific Win32/Linux calls and UI wiring; move those only with the relevant adapter and lifecycle validation. Do not assume every `not(windows)` branch is portable Unix code or apply Linux credentials, /proc or XDG behavior to Darwin.

### Cargo release profile is aggressive-size, not aggressive-speed
```toml
[profile.release]
lto = true              # full link-time optimization
codegen-units = 1       # single unit, slowest build, smallest binary
opt-level = "s"         # optimize for size (not "z", not "3")
strip = true            # remove symbols + debug info
panic = "abort"         # no unwinding tables
```
These are the root Cargo defaults. Current desktop Release jobs override LTO to thin and codegen-units to 8; record the effective configuration when comparing binaries or performance. **Do not change `opt-level` to `"3"` or add release incremental compilation as part of structural cleanup.** Any profile change needs its own measurements.

## COMMANDS

```bash
# Frontend dev (Vite, port 1420)
pnpm run dev

# Full Tauri dev (frontend + Rust binary)
pnpm run tauri:dev

# Linux dev with services disabled (faster iteration)
pnpm run tauri:dev:safe

# Type check + production frontend build (outputs dist/web/)
pnpm run build

# Full Tauri production build (deb/AppImage/rpm/NSIS)
pnpm run tauri:build

# Windows portable ZIP (cross-platform PowerShell script)
pnpm run build:portable

# i18n key audit (zh→en/tw consistency)
pnpm run i18n:check

# Rust checks from the repository root; native dependencies are required
cargo test --locked -p dzc-core --lib
cargo test --locked --workspace --all-targets

```

## NOTES

### Where things can go wrong
- **`dist/web/` must exist** before `pnpm run tauri:build` — the `beforeBuildCommand` runs `pnpm run build` but Cargo doesn't fail visibly if frontendDist is missing. Build will succeed with an empty window.
- **Build output is not acceptance evidence.** Sidecars, packaged binaries and test executables must have the expected source/profile/feature identity. Diagnose a demonstrated cache or staging mismatch before cleaning targets; a cache hit does not prove the candidate was tested.
- **WebView2 `--disable-gpu` is browser-startup-only.** Cannot hot-toggle. Changing the setting requires `app.gpu_switcher::apply_gpu_disable_env()` then a webview recreate.
- **Tauri's `WebviewWindow::destroy()` is async (message-based).** The `main` label isn't released synchronously. `idle_destroyer::recreate_main_window()` polls up to 200ms before rebuilding via `WebviewWindowBuilder::from_config`. Don't shortcut this.
- **Window resize save is debounced** (250ms in `setup.rs::persist_window_size`). Don't read `LAST_WINDOW_SIZE` from another thread without going through `app_handle.state::<DbState>()`.

### AI-assistant development artifacts
The repository was developed with AI coding assistants. Artifacts under `.trae/`, `.opencode/`, `.omo/` are session-local and gitignored. AGENTS.md is the committed knowledge base. CLAUDE.md and other ephemeral notes are not tracked.

### Commit message style
Conventional Commits in Chinese: `feat(slim): ...`, `fix(slim): ...`, `docs: ...`. The `(slim)` scope refers to the memory-pressure reduction workstream (idle destroyer + GPU switcher + low-memory webview target). When touching memory-management code, use the `slim` scope.

## SETTINGS KEYS (frozen + 5 new in roadmap-2026)

The following existing settings keys are FROZEN — do not rename, change type, or remove. New features ADD new keys; they do not modify existing ones (G15).

| Key | Default | Type | Scope | 用途 |
|---|---|---|---|---|
| `app.quick_paste_hotkey` | `"Ctrl+Shift+V"` | string | `hotkey_cmd` | 4.3 Quick-Paste 浮层 |
| `app.classification_enabled` | `true` | bool | `settings_cmd` | 4.4 智能分类总开关 |
| `app.hidden_filter_chips` | `""` | string | `settings_cmd` | 4.4 隐藏的 chip CSV |
| `app.screenshot_enabled` | `true` | bool | `settings_cmd` | 5.1 区域截图 |
| `app.ocr_enabled` | `true` | bool | `settings_cmd` | 5.2 OCR |
