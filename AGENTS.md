# PROJECT KNOWLEDGE BASE

**Generated:** 2026-06-15
**Commit:** 2eb3edf
**Branch:** slim

## OVERVIEW
TieZ (tiez-clipboard) — a Tauri 2 desktop clipboard manager for Windows and Linux. Rust backend + React 19 + TypeScript frontend. Fork of [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard) that dropped macOS and all networking features, added Linux X11 support, and built a custom CSS theme engine.

## STRUCTURE
```
tiez-clipboard/
├── src/                  # React 19 frontend (80 .ts/.tsx files)
│   ├── features/         # Feature-sliced UI: app/clipboard/emoji/settings/tag
│   ├── shared/           # Cross-cutting hooks, lib, types, components
│   ├── styles/           # Custom CSS theme engine (tokens/theme-core/themes/components)
│   ├── locales/          # i18n: en.ts / zh.ts / tw.ts (zh is base)
│   ├── App.tsx           # 772-line root composer; orchestrates ~30 hooks
│   └── main.tsx          # Entry — preloads theme CSS, multi-window routing
├── src-tauri/            # Rust backend (58 .rs files)
│   ├── src/app/          # App subsystem: setup, window_manager, idle_destroyer, gpu_switcher, webview_memory, hooks, commands
│   ├── src/services/     # Business: clipboard_ops, content_handler, paste_queue, encryption_queue, clipboard pipeline (subdir)
│   ├── src/infrastructure/ # Platform abstraction: windows_api, linux_api, repository, encryption
│   ├── src/domain/       # Domain models
│   ├── global_state.rs   # 30+ process-global atomics (WINDOW_PINNED, LAST_HIDDEN_TIMESTAMP, lifecycle state, etc.)
│   └── app_state.rs      # Tauri-managed state (SettingsState, SessionHistory, PasteQueue, EncryptionQueueState)
├── scripts/i18n-check.mjs # Custom i18n key auditor (zh→en/tw consistency)
├── public/               # Static assets (emoji-data.json)
├── docs/                 # Upstream-fork READMEs (en-US, zh-CN)
├── src-tauri/icons/      # App + tray icons (Win/Linux/Android/iOS variants)
├── src-tauri/capabilities/ # Tauri 2 permissions (main-capability + gen/)
├── src-tauri/nsis/       # NSIS uninstall hooks (Windows installer)
└── src-tauri/tauri.conf.json # Window/CSP/bundle config
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Add Tauri command | `src-tauri/src/app/commands/*.rs` + register in `main.rs` invoke_handler | Pattern: split by domain (clipboard, settings, hotkey, file, ui, system, history) |
| Add React UI | `src/features/<feature>/components/` + `hooks/` if stateful | Each feature is self-contained; re-export via barrel only if needed |
| Add Rust platform code | `src-tauri/src/infrastructure/{windows_api,linux_api}/` | NEVER touch `app/` from platform code — it goes the other way |
| Modify theme | `src/styles/themes/` for variant, `src/styles/tokens/` for vars, `src/styles/theme-core/` for cross-theme shared rules | Tokens are pure CSS custom properties; themes reference tokens |
| Add i18n key | Edit `src/locales/zh.ts` first, then `en.ts`/`tw.ts`, then run `npm run i18n:check` | `zh.ts` is the canonical source |
| Configure window/security | `src-tauri/tauri.conf.json` + `src-tauri/capabilities/default.json` | Window starts `visible: false, focusable: false, transparent: true` |
| Test pure Rust logic | Inline `#[cfg(test)] mod tests` in source file | 9 files have inline tests; no integration tests, no mock framework |

## CONVENTIONS (project-specific deviations)

### Multi-window via URL routing (NOT separate HTML entries)
`src/main.tsx` checks `?window=compact-preview` and renders `<CompactPreviewWindow />` vs `<App />` in a single bundle. Adding a new window = add URL param branch, not a new HTML file.

### Theme preloading before React hydration
`main.tsx` reads `localStorage.tiez_theme` synchronously, then `import.meta.glob("./styles/themes/*.css")` lazy-loads the chosen CSS **before** React renders. Avoids flash of default theme. **Never inline theme CSS** — keep it in `src/styles/themes/`.

### Feature-sliced frontend, no global state library
Each feature owns its hooks. No Redux/Zustand/Jotai. Cross-feature state flows via props or custom events emitted from Tauri (`app.emit(...)` ↔ `listen(...)` on the frontend).

### Rust backend organized by concern, not layer
- `app/` — bootstrap, command dispatching, window lifecycle, hooks
- `services/` — business logic (clipboard ops, paste queue, encryption)
- `infrastructure/` — OS-specific platform code (only place with `cfg(target_os = ...)` blocks touching syscalls)
- `domain/` — pure data models, no IO
- `global_state.rs` — process-wide atomics for cross-thread coordination (NEVER use a Mutex here for hot-path data)

### Single gitignored pattern: AI-tool artifacts
`.gitignore` excludes `.trae/`, `.opencode/`, `.omo/`, `CLAUDE.md`, `AGENTS.md`. These are session-local. **The committed AGENTS.md is the curated knowledge base; CLAUDE.md / session-specific notes are not committed.**

### Manual Vite chunk splitting
`vite.config.ts` splits output by feature (`settings`, `tag`, `emoji`, `compact-preview`) and by vendor (`vendor-react-select`, `vendor-motion`, `vendor-virtuoso`, `vendor-tauri`, `vendor-react`). When adding a heavy new feature, add a `manualChunks` entry to keep cold-start parse time bounded.

### Linux service opt-out via env var
`TIEZ_DISABLE_LINUX_SERVICES=window_tracker,edge_docking,mouse_hotkey` skips three Linux-specific background services. Used in `npm run tauri:dev:safe`. Pattern: any new Linux service must support this kill switch.

## ANTI-PATTERNS (THIS PROJECT)

1. **NO `TODO` / `FIXME` / `HACK` / `WORKAROUND` / `XXX` comments.** Project enforces zero tech-debt markers. The only `DO NOT` comment in the entire codebase is at `src/features/settings/components/groups/DataSettingsGroup.tsx:42` (explains deferred DB copy). Any new such marker = immediate review rejection.

2. **NO `as any` / `@ts-ignore` / `@ts-expect-error` / `// @ts-nocheck`.** `tsconfig.json` runs `strict + noUnusedLocals + noUnusedParameters`. The Rust side has zero `unsafe` outside of clearly-bounded Win32/Linux syscall blocks (see `src-tauri/src/infrastructure/`).

3. **NO committing `dist/` or `src-tauri/target/`.** Despite these appearing in the working tree, they are gitignored. Build output must NEVER be committed.

4. **NO macOS code.** `cfg(target_os = "macos")` is forbidden. README states macOS support was deliberately removed. Any re-introduction = fork principle violation.

5. **NO networking features.** Cloud sync, MQTT, AI assistant — all removed per fork. `services/` has no HTTP client, no WebSocket, no DNS. `tauri.conf.json` external URL whitelist is exactly 2 origins (`github.com/qiyuanhuakai/dezirclip`, `github.com/jimuzhe/tie-z`).

6. **NO inline theme CSS.** All styles must live under `src/styles/`. No `<style>` blocks in components.

7. **NO cargo `[dev-dependencies]` section.** All tests use inline `#[cfg(test)] mod tests` with std-only assertions. No `mockall`, no `rstest`, no `criterion`.

8. **NO frontend test framework EXCEPT scoped vitest exemption for NEW pure-logic hooks/utils only.** 已有代码、UI 组件、UI hooks 禁止添加测试。AGENTS.md 修订日期 2026-06-17，附 §roadmap-2026 G16 guardrail。

9. **NO ESLint / Prettier / rustfmt config.** Project relies on `tsc --strict` + `cargo clippy` (not configured either) + code review. Adding these is a deliberate decision, not an oversight.

10. **NO CI workflows.** `.github/` contains issue templates only. No `.github/workflows/*.yml`. All builds are local via npm scripts.

## UNIQUE STYLES

### Tailwind-free, hand-rolled CSS architecture
- `src/styles/tokens/` — CSS custom properties (`global.tokens.css`, `theme.tokens.css`, `mode.tokens.css`)
- `src/styles/themes/` — full theme variants (`retro`, `mica`, `acrylic`, `macos`, `scifi`, `liquid-glass`)
- `src/styles/theme-core/` — cross-theme shared rules (`*.shell.shared.css`, `*.dialog.shared.css`)
- `src/styles/components/` — per-component CSS (`clipboard.css`, `settings.css`, `tags.css`, etc.)
- Theme switch is runtime-only; no build-time theme stripping

### WebView2 memory hint + idle-destroy as the memory-pressure strategy
The fork's defining feature. NOT `--disable-gpu` (that breaks rendering quality). Instead:
1. `app/webview_memory.rs` calls `ICoreWebView2_19::SetMemoryUsageTargetLevel(LOW)` via Tauri `with_webview` whenever the window hides
2. `app/idle_destroyer.rs` runs a background thread that destroys the webview after N seconds of inactivity (default 60s, range 5–3600)
3. `app/gpu_switcher.rs` optionally collapses the WebView2 GPU process entirely via `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (user-toggleable via `app.disable_webview_gpu` setting, requires webview recreate to take effect)

The four-atomics state machine in `global_state.rs` (LAST_HIDDEN_TIMESTAMP, WINDOW_LIFECYCLE, IS_DESTROYED, RECREATE_PENDING) coordinates the destroy/recreate cycle. **Never read/modify these without understanding the CAS ordering rules** — see `src-tauri/src/app/idle_destroyer.rs`.

### Platform-conditional Rust, no `cfg` blocks in `app/`
`src-tauri/src/app/` is platform-agnostic orchestration. All `#[cfg(target_os = "windows")]` / `#[cfg(target_os = "linux")]` blocks touching syscalls live in `src-tauri/src/infrastructure/{windows_api,linux_api}/`. `app/hooks/mod.rs` has Win32 blocks because the hooks themselves are Win32-only; this is the **one** exception.

### Cargo release profile is aggressive-size, not aggressive-speed
```toml
[profile.release]
lto = true              # full link-time optimization
codegen-units = 1       # single unit, slowest build, smallest binary
opt-level = "s"         # optimize for size (not "z", not "3")
strip = true            # remove symbols + debug info
panic = "abort"         # no unwinding tables
```
Result: ~10–20% smaller binary. **Do not change `opt-level` to "3"** — that regresses size. Do not add `incremental = true` to release — it defeats LTO.

## COMMANDS

```bash
# Frontend dev (Vite, port 1420)
npm run dev

# Full Tauri dev (frontend + Rust binary)
npm run tauri:dev

# Linux dev with services disabled (faster iteration)
npm run tauri:dev:safe

# Type check + production frontend build (outputs dist/web/)
npm run build

# Full Tauri production build (deb/AppImage/rpm/NSIS)
npm run tauri:build

# Windows portable ZIP (cross-platform PowerShell script)
npm run build:portable

# i18n key audit (zh→en/tw consistency)
npm run i18n:check

# Rust tests (inline #[cfg(test)] modules)
cd src-tauri && cargo test

# Cleanup after Cargo dep-version stalemates
cd src-tauri && cargo clean -p <broken-crate>

# Frontend E2E (currently unconfigured — would fail)
npm run test:e2e
```

## NOTES

### Where things can go wrong
- **`dist/web/` must exist** before `npm run tauri:build` — the `beforeBuildCommand` runs `npm run build` but Cargo doesn't fail visibly if frontendDist is missing. Build will succeed with an empty window.
- **Cargo caches failed compiles.** After changing webview2-com or windows versions, run `cargo clean -p windows-future` before retrying. Otherwise a stale error persists across dep changes.
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