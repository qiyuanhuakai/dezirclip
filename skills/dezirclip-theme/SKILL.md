---
name: dezirclip-theme
description: Create or revise full importable CSS themes for DezirClip, including materials, layout, geometry and interaction states. Use for DezirClip theme packages and light/dark styles; not for general website styling.
---

# DezirClip themes

Deliver a local UTF-8 `.theme.json` package for DezirClip → Settings → Appearance → Import theme. The application offers built-in themes and imported themes, with no theme editor. A theme is a complete CSS design, not a color preset: consider materials, geometry, borders, shadows, typography, density, focus, hover, selection and dialogs.

Start with [Forest](assets/forest.theme.json). Preserve the requested style and current authorization. Write to the requested destination, or the current task's output/workspace folder. Applying a theme is part of the task only when requested.

## Package contract

- `format: "dezirclip-theme"`, `version: 1`.
- `id`: 1–48 characters matching `^[a-z0-9]+(?:-[a-z0-9]+)*$`; imports refuse an existing ID, so give revisions a new ID or remove the old imported theme first. Stored selection is `custom:<id>`.
- `name`: 1–80 printable Unicode characters. Optional `author` (80) and `description` (240). Unknown fields are rejected.
- `base`: `mica`, `acrylic`, `liquid-glass`, `retro`, `macos` or `scifi`, used for the native window material. Imported CSS owns the visual design and does not inherit that built-in theme's selectors. `macos` is a visual name, not platform support.
- `css`: nonempty stylesheet text. Maximum package size 256 KiB; at most 32 imported themes.

Build the package with a JSON serializer so CSS newlines and quotes are escaped correctly. Do not write a palette-only `modes` field.

## Write the design

The runtime adds `.theme-custom` to both `html` and `body`, and `.light-mode` / `.dark-mode` for the current mode. Scope your rules to `.theme-custom`; define root variables with `:root.theme-custom` and provide a `.dark-mode` counterpart. Native material selection and the app's `--surface-opacity` control remain separate.

New shared controls consume the following semantic variables. Define them in both modes, then add CSS for the desired shapes, materials and states. Legacy controls still use aliases such as `--bg-window`, `--text-primary`, `--border-dark`, `--accent-color` and `--icon-btn-*`; the starter demonstrates those mappings.

| Role | Tokens |
| --- | --- |
| Surfaces | `--dc-surface-window`, `--dc-surface-panel`, `--dc-surface-raised`, `--dc-surface-input`, `--dc-surface-hover`, `--dc-surface-active` |
| Text | `--dc-text-primary`, `--dc-text-secondary`, `--dc-text-muted`, `--dc-text-on-accent` |
| Borders | `--dc-border`, `--dc-border-strong` |
| Accent | `--dc-accent`, `--dc-accent-hover`, `--dc-accent-soft` |
| Status | `--dc-success`, `--dc-warning`, `--dc-danger`, `--dc-info` |
| Geometry | `--dc-radius` (a CSS length) |

Use shared component/state variables before adding selector overrides. Do not solve every control with `!important`. Preserve user-assigned tag colors (`data-tag` elements); do not flatten them into one accent. Keep list virtualization and fixed/flexible container sizing intact. Prefer window/panel-level material; avoid a separate backdrop blur on every history or tag card. Honor `prefers-reduced-motion` where animation is added.

No external resources: do not use `@import`, `@font-face`, `@namespace`, `@document`, `url()`, `src()`, `image-set()` or vendor variants, including escaped spellings. No remote/local/data URLs, scripts or image/font assets. Gradients, shadows, CSS shapes, `calc()`, custom properties, media/container rules and interaction states can express the design locally.

## Validate and review

Use the application's authoritative parser without importing or opening history:

```text
dzc theme validate /absolute/path/to/my.theme.json --json
```

If installed `dzc` predates this command, use a current source build or matching release. In a source checkout, `cargo run --manifest-path src-tauri/Cargo.toml --no-default-features --bin dzc -- theme validate /absolute/path/to/my.theme.json --json` runs the same parser. Never claim a JSON-only check proves CSS is safe. Contract sources: `src/shared/lib/themePackage.ts` and `src-tauri/src/domain/theme.rs`.

When application access and applying the theme are authorized, inspect history, tags, settings, menus, confirmation dialogs and Quick Paste in both modes, including hover/selected/focus states and transparency endpoints. Check readable contrast, clipping and narrow-window behavior. Otherwise deliver the package and the short import path. Browser screenshots demonstrate appearance; do not describe them as native desktop GPU measurements.
