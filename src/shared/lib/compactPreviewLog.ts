const COMPACT_PREVIEW_DEBUG = false;
const RICH_PREVIEW_DEBUG = import.meta.env.DEV;

export const compactPreviewLog = (...args: unknown[]) => {
    if (!COMPACT_PREVIEW_DEBUG) return;
    const ts = new Date().toISOString();
    console.log(`[CompactPreview][${ts}]`, ...args);
};

export const richPreviewFailureLog = (stage: string, detail?: Record<string, unknown>) => {
    if (!RICH_PREVIEW_DEBUG) return;
    console.warn("[RichTextPreview]", stage, detail || {});
};
