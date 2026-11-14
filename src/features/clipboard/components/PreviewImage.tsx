import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { boundedPreviewZoom, fitPreviewZoom, fittedAxisPan, zoomPreviewScroll } from "./previewImageGeometry";

interface PreviewImageProps {
    src: string;
    zoom: number | "fit";
    onZoomChange: (zoom: number | "fit") => void;
    onFitZoom: (zoom: number) => void;
    onLoad: () => void;
    hoverBounds?: { width: number; height: number };
}

export function PreviewImage({ src, zoom, onZoomChange, onFitZoom, onLoad, hoverBounds }: PreviewImageProps) {
    const viewport = useRef<HTMLDivElement>(null);
    const [natural, setNatural] = useState({ width: 1, height: 1 });
    const [loadedSource, setLoadedSource] = useState<string | null>(null);
    useLayoutEffect(() => { if (loadedSource === src) onLoad(); }, [loadedSource, natural, src]);
    const [fit, setFit] = useState(1);
    const picture = useRef<HTMLImageElement>(null);
    const pan = useRef({ x: 0, y: 0 });
    const movePicture = (x: number, y: number) => {
        pan.current = { x, y };
        if (picture.current) picture.current.style.transform = x || y ? `translate(${x}px, ${y}px)` : "";
    };
    const drag = useRef<{ id: number; x: number; y: number; left: number; top: number; panX: number; panY: number } | null>(null);
    const anchor = useRef<{ x: number; y: number } | null>(null);
    const previous = useRef({ source: src, scale: 1 });
    const [dragging, setDragging] = useState(false);
    useLayoutEffect(() => {
        const node = viewport.current;
        if (!node) return;
        const measure = () => {
            const next = fitPreviewZoom({ width: node.clientWidth, height: node.clientHeight }, natural);
            setFit(next); onFitZoom(next);
        };
        const observer = new ResizeObserver(measure);
        observer.observe(node); measure();
        return () => observer.disconnect();
    }, [natural, onFitZoom]);
    useEffect(() => {
        const node = viewport.current;
        if (!node || hoverBounds) return;
        const wheel = (event: WheelEvent) => {
            if (event.deltaY === 0) return;
            event.preventDefault();
            const bounds = node.getBoundingClientRect();
            anchor.current = { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
            const current = zoom === "fit" ? fit : zoom;
            onZoomChange(boundedPreviewZoom(current * (event.deltaY < 0 ? 1.2 : 1 / 1.2)));
        };
        node.addEventListener("wheel", wheel, { passive: false });
        return () => node.removeEventListener("wheel", wheel);
    }, [zoom, fit, onZoomChange, hoverBounds]);
    const scale = zoom === "fit" ? fit : zoom;
    useLayoutEffect(() => {
        const node = viewport.current;
        if (!node) return;
        if (previous.current.source === src) {
            const next = zoomPreviewScroll({ width: node.clientWidth, height: node.clientHeight }, natural,
                { x: node.scrollLeft, y: node.scrollTop }, anchor.current ?? { x: node.clientWidth / 2, y: node.clientHeight / 2 },
                previous.current.scale, scale, pan.current);
            node.scrollLeft = next.x;
            node.scrollTop = next.y;
        } else { node.scrollLeft = 0; node.scrollTop = 0; }
        movePicture(0, 0);
        anchor.current = null;
        previous.current = { source: src, scale };
    }, [scale, natural, src]);
    const hoverFit = hoverBounds && fitPreviewZoom(hoverBounds, natural);
    const hoverSize = hoverFit === undefined ? undefined : {
        width: Math.round(natural.width * hoverFit), height: Math.round(natural.height * hoverFit),
    };
    return <div style={hoverSize} data-ready={loadedSource === src} className={`preview-image-viewport${dragging ? " dragging" : ""}`} ref={viewport}
        onPointerDown={event => {
            if (event.button !== 0) return;
            const node = event.currentTarget;
            drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY, left: node.scrollLeft, top: node.scrollTop, panX: pan.current.x, panY: pan.current.y };
            node.setPointerCapture(event.pointerId);
            setDragging(true);
            event.preventDefault();
        }} onPointerMove={event => {
            const active = drag.current;
            if (!active || active.id !== event.pointerId) return;
            const node = event.currentTarget;
            const dx = event.clientX - active.x;
            const dy = event.clientY - active.y;
            node.scrollLeft = active.left - dx;
            node.scrollTop = active.top - dy;
            movePicture(fittedAxisPan(node.clientWidth, Math.round(natural.width * scale), active.panX + dx),
                fittedAxisPan(node.clientHeight, Math.round(natural.height * scale), active.panY + dy));
        }} onPointerUp={event => {
            if (drag.current?.id !== event.pointerId) return;
            drag.current = null;
            if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
            setDragging(false);
        }} onLostPointerCapture={() => { drag.current = null; setDragging(false); }}>
        <div className="preview-image-canvas" style={{ width: Math.round(natural.width * scale), height: Math.round(natural.height * scale) }}><img ref={picture} src={src} alt="" draggable={false}
            width={Math.round(natural.width * scale)} height={Math.round(natural.height * scale)}
            onLoad={event => {
                setNatural({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight });
                setLoadedSource(src);
            }} onError={() => setLoadedSource(src)} /></div>
    </div>;
}
