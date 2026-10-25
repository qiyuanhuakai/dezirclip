export const isPinnedPreviewLabel = (label: string): boolean =>
    /^pinned-preview-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(label);
