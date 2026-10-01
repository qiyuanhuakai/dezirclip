// Applies the user-controlled appearance overrides to the document.
//
// Every window is its own webview with its own document, so the overrides have
// to be written in each one. The main window calls this from its settings hook;
// the auxiliary windows (compact preview, quick paste, region select) call it
// from `useAppearanceOverrides`, which reads the same values from the backend.

const overrideTargets = (): HTMLElement[] => [document.documentElement, document.body];

const writeToken = (cssVar: string, value: string | undefined): void => {
  const hasValue = typeof value === "string" && value.trim().length > 0;
  for (const target of overrideTargets()) {
    if (hasValue) {
      target.style.setProperty(cssVar, value as string);
    } else {
      target.style.removeProperty(cssVar);
    }
  }
};

/** Quote a family name so names with spaces, digits, or punctuation stay valid. */
const quoteFamily = (family: string): string =>
  `"${family.trim().replace(/(["\\])/g, "\\$1")}"`;

/** Write or clear the font family overrides. Empty values fall back to the theme. */
export const applyFontOverrides = (fontMain: string, fontMono: string): void => {
  // Themes declare `--font-main` / `--font-mono` on both `:root.theme-*` and
  // `body.theme-*`. A declaration on `body` shadows whatever `html` inherits, so
  // an override that only lands on `documentElement` loses to the theme for the
  // entire rendered subtree. Inline styles outrank stylesheet rules on the
  // element they sit on, which is why both elements get the value.
  writeToken("--font-main", fontMain.trim() ? quoteFamily(fontMain) : undefined);
  writeToken("--font-mono", fontMono.trim() ? quoteFamily(fontMono) : undefined);
};

/**
 * Write the absolute shell alpha (0..1) that `--surface-shell-root-bg` and
 * `--glass-shell-bg` resolve against.
 */
export const applySurfaceOpacity = (surfaceOpacity: number): void => {
  if (!Number.isFinite(surfaceOpacity)) return;
  const alpha = Math.min(1, Math.max(0, surfaceOpacity / 100));
  document.documentElement.style.setProperty("--surface-opacity", String(alpha));
};

/** Write the clipboard list font size overrides used by item and tag rendering. */
export const applyClipboardFontSizes = (itemPx: number, tagPx: number): void => {
  if (Number.isFinite(itemPx)) {
    document.documentElement.style.setProperty("--clipboard-item-font-size", `${itemPx}px`);
  }
  if (Number.isFinite(tagPx)) {
    document.documentElement.style.setProperty("--clipboard-tag-font-size", `${tagPx}px`);
  }
};
