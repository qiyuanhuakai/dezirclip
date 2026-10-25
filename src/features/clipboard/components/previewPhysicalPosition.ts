import type { CompactPreviewPoint } from "./compactPreviewPosition";

/** Tauri's physical window positions cross an i32 IPC boundary. */
export function previewPhysicalPosition(point: CompactPreviewPoint): CompactPreviewPoint {
    const x = Math.round(point.x);
    const y = Math.round(point.y);
    if (!Number.isFinite(x) || !Number.isFinite(y)
        || x < -2_147_483_648 || x > 2_147_483_647
        || y < -2_147_483_648 || y > 2_147_483_647) throw new Error("PreviewPositionUnavailable");
    return { x, y };
}
