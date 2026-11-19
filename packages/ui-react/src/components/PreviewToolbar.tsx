import { useId, type PointerEvent } from "react";
import { Blend, GripHorizontal, Minus, Pin, Plus, Scan, X } from "lucide-react";

export type PreviewZoom = number | "fit";

type ToolbarLabels = {
    preview_toolbar_actual: string;
    preview_toolbar_close: string;
    preview_toolbar_drag: string;
    preview_toolbar_fit: string;
    preview_toolbar_fit_label: string;
    preview_toolbar_opacity: string;
    preview_toolbar_pin: string;
    preview_toolbar_title: string;
    preview_toolbar_zoom: string;
    preview_toolbar_zoom_in: string;
    preview_toolbar_zoom_out: string;
};

export interface PreviewToolbarProps {
    pinned: boolean;
    pinPending?: boolean;
    opacity: number;
    image: boolean;
    zoom: PreviewZoom;
    fitZoom?: number;
    labels: ToolbarLabels;
    dragRegionAttributes?: Record<string, string | boolean>;
    onPin: (pinned: boolean) => void;
    onClose: () => void;
    onOpacityChange: (opacity: number) => void;
    onZoomChange: (zoom: PreviewZoom) => void;
    onDrag?: () => void;
}

const boundedZoom = (value: number) => Math.min(8, Math.max(0.1, Number.isFinite(value) ? value : 1));

export function PreviewToolbar({ pinned, pinPending = false, opacity, image, zoom, fitZoom = 1, labels: text, dragRegionAttributes,
    onPin, onClose, onOpacityChange, onZoomChange, onDrag }: PreviewToolbarProps) {
    const opacityId = useId();
    const percentage = Math.round(Math.min(100, Math.max(0, Number.isFinite(opacity) ? opacity : 100)));
    const factor = boundedZoom(zoom === "fit" ? fitZoom : zoom);
    const zoomBy = (multiplier: number) => onZoomChange(Math.round(boundedZoom(factor * multiplier) * 10000) / 10000);
    const drag = (event: PointerEvent<HTMLDivElement>) => {
        if (event.button !== 0 || !onDrag) return;
        event.preventDefault();
        onDrag();
    };
    return <div className="preview-toolbar" role="toolbar" aria-label={text.preview_toolbar_title}>
        <div className="preview-toolbar-title-row">
            <div className="preview-toolbar-drag" {...(pinned && !onDrag ? dragRegionAttributes : undefined)}
                onPointerDown={pinned ? drag : undefined} title={pinned ? text.preview_toolbar_drag : undefined}>
                <GripHorizontal size={14} aria-hidden="true" />
                <span>{text.preview_toolbar_title}</span>
            </div>
            <div className="preview-toolbar-actions">
                <button type="button" className={`btn-icon preview-toolbar-button${pinned ? " active" : ""}`}
                    title={pinned ? text.preview_toolbar_close : text.preview_toolbar_pin}
                    aria-label={pinned ? text.preview_toolbar_close : text.preview_toolbar_pin}
                    aria-pressed={pinned} disabled={pinPending} onClick={() => pinned ? onClose() : onPin(true)}><Pin size={14} /></button>
                {!pinned && <button type="button" className="btn-icon preview-toolbar-button" title={text.preview_toolbar_close}
                    aria-label={text.preview_toolbar_close} onClick={onClose}><X size={14} /></button>}
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
    </div>;
}

const directions = ["North", "NorthEast", "East", "SouthEast", "South", "SouthWest", "West", "NorthWest"] as const;
export type PreviewResizeDirection = typeof directions[number];

export function PreviewResizeHandles({ label, onResize }: { label: string; onResize: (direction: PreviewResizeDirection) => void }) {
    return <div className="preview-resize-handles" aria-hidden="true">
        {directions.map(direction => <div key={direction} className={`preview-resize-handle preview-resize-${direction.toLowerCase()}`}
            title={label} onPointerDown={event => {
                if (event.button !== 0) return;
                event.preventDefault();
                event.stopPropagation();
                onResize(direction);
            }} />)}
    </div>;
}
