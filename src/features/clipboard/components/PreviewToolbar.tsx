import { useId, type PointerEvent } from "react";
import { Blend, GripHorizontal, Minus, Pin, Plus, Scan, X } from "lucide-react";
import type { Window as TauriWindow } from "@tauri-apps/api/window";
import { translations } from "../../../locales";
import type { Locale } from "../../../shared/types";

export type PreviewZoom = number | "fit";
export type PreviewResizeDirection = Parameters<TauriWindow["startResizeDragging"]>[0];

export interface PreviewToolbarProps {
    pinned: boolean;
    pinPending?: boolean;
    opacity: number;
    image: boolean;
    zoom: PreviewZoom;
    fitZoom?: number;
    language: Locale;
    onPin: (pinned: boolean) => void;
    onClose: () => void;
    onOpacityChange: (opacity: number) => void;
    onZoomChange: (zoom: PreviewZoom) => void;
    onDrag?: () => void;
}

const boundedZoom = (value: number) => Math.min(8, Math.max(0.1, Number.isFinite(value) ? value : 1));

export function PreviewToolbar({ pinned, pinPending = false, opacity, image, zoom, fitZoom = 1, language,
    onPin, onClose, onOpacityChange, onZoomChange, onDrag }: PreviewToolbarProps) {
    const text = translations[language];
    const opacityId = useId();
    const percentage = Math.round(Math.min(100, Math.max(0, Number.isFinite(opacity) ? opacity : 100)));
    const factor = boundedZoom(zoom === "fit" ? fitZoom : zoom);
    const zoomBy = (multiplier: number) => onZoomChange(Math.round(boundedZoom(factor * multiplier) * 10000) / 10000);
    const drag = (event: PointerEvent<HTMLDivElement>) => {
        if (event.button !== 0 || !onDrag) return;
        event.preventDefault();
        onDrag();
    };
    return <header className="preview-toolbar">
        <div className="preview-toolbar-title-row">
            <div className="preview-toolbar-drag" data-tauri-drag-region={pinned && !onDrag ? "true" : undefined}
                onPointerDown={pinned ? drag : undefined} title={pinned ? text.preview_toolbar_drag : undefined}>
                <GripHorizontal size={14} aria-hidden="true" />
                <span>{text.preview_toolbar_title}</span>
            </div>
            <div className="preview-toolbar-actions">
                <button type="button" className={`btn-icon preview-toolbar-button${pinned ? " active" : ""}`}
                    title={pinned ? text.preview_toolbar_pinned : text.preview_toolbar_pin}
                    aria-label={pinned ? text.preview_toolbar_pinned : text.preview_toolbar_pin}
                    aria-pressed={pinned} disabled={pinned || pinPending} onClick={() => onPin(true)}><Pin size={14} /></button>
                <button type="button" className="btn-icon preview-toolbar-button" title={text.preview_toolbar_close}
                    aria-label={text.preview_toolbar_close} onClick={onClose}><X size={14} /></button>
            </div>
        </div>
        <div className="preview-toolbar-controls">
            <label className="preview-toolbar-opacity" htmlFor={opacityId} title={text.preview_toolbar_opacity}>
                <Blend size={14} aria-hidden="true" />
                <input id={opacityId} type="range" min={0} max={100} step={1} value={percentage}
                    aria-label={text.preview_toolbar_opacity} aria-valuetext={`${percentage}%`}
                    onChange={event => onOpacityChange(event.currentTarget.valueAsNumber)} />
                <output htmlFor={opacityId}>{percentage}%</output>
            </label>
            {image && <div className="preview-toolbar-zoom" role="group" aria-label={text.preview_toolbar_zoom}>
                <button type="button" className="btn-icon preview-toolbar-button" title={text.preview_toolbar_zoom_out}
                    aria-label={text.preview_toolbar_zoom_out} disabled={factor <= 0.1} onClick={() => zoomBy(0.8)}><Minus size={14} /></button>
                <output className="preview-toolbar-zoom-value">{zoom === "fit" ? text.preview_toolbar_fit_label : `${Math.round(factor * 100)}%`}</output>
                <button type="button" className="btn-icon preview-toolbar-button" title={text.preview_toolbar_zoom_in}
                    aria-label={text.preview_toolbar_zoom_in} disabled={factor >= 8} onClick={() => zoomBy(1.25)}><Plus size={14} /></button>
                <button type="button" className={`btn-icon preview-toolbar-button${zoom === "fit" ? " active" : ""}`}
                    title={text.preview_toolbar_fit} aria-label={text.preview_toolbar_fit} aria-pressed={zoom === "fit"}
                    onClick={() => onZoomChange("fit")}><Scan size={14} /></button>
                <button type="button" className="btn-icon preview-toolbar-button preview-toolbar-actual"
                    title={text.preview_toolbar_actual} aria-label={text.preview_toolbar_actual}
                    onClick={() => onZoomChange(1)}>100%</button>
            </div>}
        </div>
    </header>;
}

const directions: PreviewResizeDirection[] = ["North", "NorthEast", "East", "SouthEast", "South", "SouthWest", "West", "NorthWest"];

export function PreviewResizeHandles({ language, onResize }: { language: Locale; onResize: (direction: PreviewResizeDirection) => void }) {
    return <div className="preview-resize-handles" aria-hidden="true">
        {directions.map(direction => <div key={direction} className={`preview-resize-handle preview-resize-${direction.toLowerCase()}`}
            title={translations[language].preview_toolbar_resize} onPointerDown={event => {
                if (event.button !== 0) return;
                event.preventDefault();
                event.stopPropagation();
                onResize(direction);
            }} />)}
    </div>;
}
