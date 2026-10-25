import { emitTo } from "@tauri-apps/api/event";
import { currentMonitor, getCurrentWindow, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import type { PreviewLayout, PreviewRequest } from "./compactPreviewProtocol";
import { previewPhysicalPosition } from "./previewPhysicalPosition";

/** A pinned preview owns its WebView and survives the main WebView's idle destruction. */
export async function openPinnedPreview(request: PreviewRequest, origin: PhysicalPosition, valid: () => boolean): Promise<void> {
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    if (!valid()) return;
    const label = `pinned-preview-${crypto.randomUUID()}`;
    const requestId = `${label}:1`;
    const stops: Array<() => void> = [];
    let mounted = false;
    let reportLayout: (layout: PreviewLayout) => void = () => {};
    let rejectLayout: (error: Error) => void = () => {};
    let notifyMounted = () => {};
    const ready = new Promise<void>(resolve => { notifyMounted = resolve; });
    const prepared = new Promise<PreviewLayout>((resolve, reject) => { reportLayout = resolve; rejectLayout = reject; });
    void prepared.catch(() => undefined);
    const deadline = setTimeout(() => rejectLayout(new Error("PinnedPreviewTimeout")), 8000);
    let owned: InstanceType<typeof WebviewWindow> | null = null;
    let finished = false;
    let invalidated = false;
    const requireCurrent = () => { if (invalidated || !valid()) throw new Error("PreviewRequestExpired"); };
    const retainStop = (stop: () => void) => { if (finished) stop(); else stops.push(stop); };
    try {
        stops.push(await getCurrentWindow().listen<{ label: string }>("compact-preview-mounted", event => {
            if (event.payload?.label === label) { mounted = true; notifyMounted(); }
        }));
        requireCurrent();
        stops.push(await getCurrentWindow().listen<PreviewLayout>("compact-preview-layout", event => {
            if (event.payload?.requestId === requestId) reportLayout(event.payload);
        }));
        requireCurrent();
        stops.push(await getCurrentWindow().listen<{ requestId: string }>("compact-preview-failed", event => {
            if (event.payload?.requestId === requestId) {
                invalidated = true;
                rejectLayout(new Error("PinnedPreviewUnavailable"));
            }
        }));
        requireCurrent();
        owned = new WebviewWindow(label, {
            url: "index.html?window=compact-preview&pinned=true", title: "DezirClip",
            width: request.maxWidth, height: request.maxHeight,
            minWidth: Math.min(240, request.maxWidth), minHeight: Math.min(160, request.maxHeight),
            decorations: false, transparent: true, resizable: true, skipTaskbar: true,
            alwaysOnTop: true, visible: false, focus: false, focusable: true, shadow: false
        });
        const window = owned;
        // Creation events are local to this object and can arrive before a native listener registers.
        const created = new Promise<void>((resolve, reject) => {
            void window.once("tauri://created", () => resolve()).then(retainStop, reject);
            void window.once("tauri://error", () => reject(new Error("PinnedPreviewCreateFailed"))).then(retainStop, reject);
        });
        void created.catch(() => undefined);
        void window.once("tauri://destroyed", () => rejectLayout(new Error("PinnedPreviewClosed")))
            .then(retainStop, rejectLayout);
        await Promise.race([
            created, prepared.then(() => {})
        ]);
        requireCurrent();
        if (!mounted) await emitTo(label, "compact-preview-probe", null);
        requireCurrent();
        await Promise.race([ready, prepared.then(() => {})]);
        requireCurrent();
        await emitTo(label, "compact-preview-update", { ...request, requestId, pinned: true });
        requireCurrent();
        const measured = await prepared;
        requireCurrent();
        if (!Number.isFinite(measured.width) || !Number.isFinite(measured.height)) throw new Error("PreviewRequestExpired");
        const monitor = await currentMonitor();
        requireCurrent();
        const scale = monitor?.scaleFactor ?? await window.scaleFactor();
        requireCurrent();
        const area = monitor?.workArea ?? { position: { x: 0, y: 0 }, size: { width: 1920, height: 1080 } };
        const size = new PhysicalSize(Math.round(Math.max(Math.min(320, request.maxWidth), Math.min(request.maxWidth, measured.width)) * scale),
            Math.round(Math.max(Math.min(180, request.maxHeight), Math.min(request.maxHeight, measured.height)) * scale));
        const position = previewPhysicalPosition({
            x: Math.max(area.position.x, Math.min(origin.x, area.position.x + area.size.width - size.width)),
            y: Math.max(area.position.y, Math.min(origin.y, area.position.y + area.size.height - size.height))
        });
        await window.setPosition(new PhysicalPosition(position.x, position.y));
        requireCurrent();
        await window.setSize(size);
        requireCurrent();
        await window.setAlwaysOnTop(true);
        requireCurrent();
        await window.show();
        requireCurrent();
        await emitTo(label, "compact-preview-presented", { requestId });
        requireCurrent();
    } catch (error) {
        if (owned) await owned.close().catch(() => undefined);
        throw error;
    } finally {
        finished = true;
        clearTimeout(deadline);
        stops.forEach(stop => stop());
    }
}
