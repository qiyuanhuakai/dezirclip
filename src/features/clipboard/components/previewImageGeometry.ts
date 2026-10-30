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
    anchor: ImagePoint, before: number, after: number): ImagePoint {
    if (!Number.isFinite(before) || !Number.isFinite(after) || before <= 0 || after <= 0) return scroll;
    const axis = (view: number, natural: number, offset: number, point: number) => {
        const start = Math.max(0, (view - natural * before) / 2);
        const end = Math.max(0, (view - natural * after) / 2);
        const imagePoint = (offset + point - start) / before;
        return Math.max(0, Math.min(Math.max(0, natural * after - view), imagePoint * after + end - point));
    };
    return { x: axis(viewport.width, image.width, scroll.x, anchor.x), y: axis(viewport.height, image.height, scroll.y, anchor.y) };
}
