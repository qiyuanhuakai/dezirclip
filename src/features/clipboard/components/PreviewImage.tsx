import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { boundedPreviewZoom, fitPreviewZoom, zoomPreviewScroll } from "./previewImageGeometry";

interface PreviewImageProps {
    src: string;
    zoom: number | "fit";
    onZoomChange: (zoom: number | "fit") => void;
    onFitZoom: (zoom: number) => void;
    onLoad: () => void;
}

export function PreviewImage({ src, zoom, onZoomChange, onFitZoom, onLoad }: PreviewImageProps) {
    const viewport = useRef<HTMLDivElement>(null);
    const [natural, setNatural] = useState({ width: 1, height: 1 });
    const [fit, setFit] = useState(1);
    const drag = useRef<{ id: number; x: number; y: number; left: number; top: number } | null>(null);
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
        if (!node) return;
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
    }, [zoom, fit, onZoomChange]);
    const scale = zoom === "fit" ? fit : zoom;
    useLayoutEffect(() => {
        const node = viewport.current;
        if (!node) return;
        if (previous.current.source === src) {
            const next = zoomPreviewScroll({ width: node.clientWidth, height: node.clientHeight }, natural,
                { x: node.scrollLeft, y: node.scrollTop }, anchor.current ?? { x: node.clientWidth / 2, y: node.clientHeight / 2 },
                previous.current.scale, scale);
            node.scrollLeft = next.x;
            node.scrollTop = next.y;
        } else { node.scrollLeft = 0; node.scrollTop = 0; }
        anchor.current = null;
        previous.current = { source: src, scale };
    }, [scale, natural, src]);
    return <div className={`preview-image-viewport${dragging ? " dragging" : ""}`} ref={viewport}
        onPointerDown={event => {
            if (event.button !== 0) return;
            const node = event.currentTarget;
            drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY, left: node.scrollLeft, top: node.scrollTop };
            node.setPointerCapture(event.pointerId);
            setDragging(true);
            event.preventDefault();
        }} onPointerMove={event => {
            const active = drag.current;
            if (!active || active.id !== event.pointerId) return;
            event.currentTarget.scrollLeft = active.left + active.x - event.clientX;
            event.currentTarget.scrollTop = active.top + active.y - event.clientY;
        }} onPointerUp={event => {
            if (drag.current?.id !== event.pointerId) return;
            drag.current = null;
            if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
            setDragging(false);
        }} onLostPointerCapture={() => { drag.current = null; setDragging(false); }}>
        <div className="preview-image-canvas" style={{ width: Math.round(natural.width * scale), height: Math.round(natural.height * scale) }}><img src={src} alt="" draggable={false}
            width={Math.round(natural.width * scale)} height={Math.round(natural.height * scale)}
            onLoad={event => {
                setNatural({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight });
                onLoad();
            }} onError={onLoad} /></div>
    </div>;
}
