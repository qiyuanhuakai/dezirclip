export const PREVIEW_KEYBOARD_KEY = "app.preview_keyboard_enabled";
export const PREVIEW_OPACITY_KEY = "app.preview_opacity";

export interface PreviewPreferences {
  keyboardEnabled: boolean;
  opacity: number;
}

export function normalizePreviewOpacity(value: number, fallback = 65): number {
  return Number.isFinite(value) ? Math.round(Math.min(100, Math.max(0, value))) : fallback;
}

const parseOpacity = (value: string | undefined, fallback: number) =>
  value?.trim() ? normalizePreviewOpacity(Number(value), fallback) : fallback;

export function readPreviewPreferences(settings: Record<string, string>): PreviewPreferences {
  const surfaceOpacity = parseOpacity(settings["app.surface_opacity"], 65);
  return {
    keyboardEnabled: settings[PREVIEW_KEYBOARD_KEY] === "true",
    opacity: parseOpacity(settings[PREVIEW_OPACITY_KEY], surfaceOpacity)
  };
}
