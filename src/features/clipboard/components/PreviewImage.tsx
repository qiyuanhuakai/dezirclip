import { useEffect, useLayoutEffect, useRef, useState } from "react";

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
    useLayoutEffect(() => {
        const node = viewport.current;
        if (!node) return;
        const measure = () => {
            const ratio = Math.min(1, node.clientWidth / natural.width, node.clientHeight / natural.height);
            const next = Math.max(0.01, ratio);
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
            if (!event.ctrlKey) return;
            event.preventDefault();
            const current = zoom === "fit" ? fit : zoom;
            onZoomChange(Math.min(8, Math.max(0.1, current * (event.deltaY < 0 ? 1.2 : 1 / 1.2))));
        };
        node.addEventListener("wheel", wheel, { passive: false });
        return () => node.removeEventListener("wheel", wheel);
    }, [zoom, fit, onZoomChange]);
    const scale = zoom === "fit" ? fit : zoom;
    return <div className="preview-image-viewport" ref={viewport}>
        <img src={src} alt="" draggable={false}
            width={Math.round(natural.width * scale)} height={Math.round(natural.height * scale)}
            onLoad={event => {
                setNatural({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight });
                onLoad();
            }} onError={onLoad} />
    </div>;
}
