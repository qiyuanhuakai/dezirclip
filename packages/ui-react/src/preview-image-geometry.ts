export type ImageSize = { width: number; height: number };
export type ImagePoint = { x: number; y: number };

export function boundedPreviewZoom(value: number): number {
    return Math.max(0.1, Math.min(8, Number.isFinite(value) ? value : 1));
}

export function fitPreviewZoom(viewport: ImageSize, image: ImageSize): number {
    if (image.width <= 0 || image.height <= 0 || viewport.width <= 0 || viewport.height <= 0) return 1;
    return Math.max(0.01, Math.min(1, viewport.width / image.width, viewport.height / image.height));
}

// Coordinates are relative to the scroll viewport; centering margins are part
// of the displayed image's origin and must be removed before scaling.
export function zoomPreviewScroll(viewport: ImageSize, image: ImageSize, scroll: ImagePoint,
    anchor: ImagePoint, before: number, after: number, pan: ImagePoint = { x: 0, y: 0 }): ImagePoint {
    if (!Number.isFinite(before) || !Number.isFinite(after) || before <= 0 || after <= 0) return scroll;
    const axis = (view: number, natural: number, offset: number, point: number, shift: number) => {
        const start = Math.max(0, (view - natural * before) / 2);
        const end = Math.max(0, (view - natural * after) / 2);
        const imagePoint = (offset + point - start - shift) / before;
        return Math.max(0, Math.min(Math.max(0, natural * after - view), imagePoint * after + end - point));
    };
    return { x: axis(viewport.width, image.width, scroll.x, anchor.x, pan.x), y: axis(viewport.height, image.height, scroll.y, anchor.y, pan.y) };
}

// A fitted axis can pan inside the viewport. Keep a grip on the image so it can always be dragged back.
export function fittedAxisPan(view: number, image: number, displacement: number): number {
    if (image > view || view <= 0 || image <= 0) return 0;
    const limit = (view + image) / 2 - Math.min(24, view, image);
    return Math.max(-limit, Math.min(limit, displacement));
}
