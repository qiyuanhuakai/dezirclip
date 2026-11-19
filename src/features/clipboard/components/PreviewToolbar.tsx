import type { Window as TauriWindow } from "@tauri-apps/api/window";
import { PreviewResizeHandles as ResizeHandles, PreviewToolbar as Toolbar, type PreviewToolbarProps as ToolbarProps } from "@deziro/ui-react/components/PreviewToolbar";
import { translations } from "../../../locales";
import type { Locale } from "../../../shared/types";

export type { PreviewZoom } from "@deziro/ui-react/components/PreviewToolbar";
export type PreviewResizeDirection = Parameters<TauriWindow["startResizeDragging"]>[0];

export type PreviewToolbarProps = Omit<ToolbarProps, "labels" | "dragRegionAttributes"> & { language: Locale };

const dragRegionAttributes = { "data-tauri-drag-region": "true" };

export function PreviewToolbar({ language, ...props }: PreviewToolbarProps) {
    return <Toolbar {...props} labels={translations[language]} dragRegionAttributes={dragRegionAttributes} />;
}

export function PreviewResizeHandles({ language, onResize }: { language: Locale; onResize: (direction: PreviewResizeDirection) => void }) {
    return <ResizeHandles label={translations[language].preview_toolbar_resize} onResize={onResize} />;
}
