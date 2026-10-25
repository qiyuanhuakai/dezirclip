import type { Locale } from "../../../shared/types";

export const COMPACT_PREVIEW_LABEL = "compact-preview";
export type PreviewRequest = {
    pinned?: boolean;
    requestId: string;
    id: number;
    privacyEpoch: number;
    expectedHostInstanceId?: string;
    allowSensitive: boolean;
    language: Locale;
    theme: string;
    colorMode: "light" | "dark";
    richTextSnapshotPreview: boolean;
    clipboardItemFontSize?: number;
    clipboardTagFontSize?: number;
    maxWidth: number;
    maxHeight: number;
};
export type PreviewLayout = { requestId: string; width: number; height: number };
