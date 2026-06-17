/**
 * Normalize a hotkey string for display.
 * "ctrl+shift+v" → "Ctrl+Shift+V"
 */
export function formatShortcut(raw: string): string {
  return raw
    .split('+')
    .map((part) => {
      const trimmed = part.trim();
      if (trimmed.length === 0) return trimmed;
      return trimmed[0].toUpperCase() + trimmed.slice(1).toLowerCase();
    })
    .join('+');
}
