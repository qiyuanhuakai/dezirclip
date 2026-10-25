import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import { currentMonitor, getCurrentWindow, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import type { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { beginPrivacyAccess, subscribePrivacyInvalidation } from "../../../shared/lib/privacySession";
import { compactPreviewLog } from "../../../shared/lib/compactPreviewLog";
import { createPreviewSessionQueue } from "./compactPreviewSession";
import { pickPreviewPosition, type CompactPreviewRect } from "./compactPreviewPosition";
import { COMPACT_PREVIEW_LABEL, type PreviewRequest, type PreviewLayout } from "./compactPreviewProtocol";
import { openPinnedPreview } from "./pinnedPreviewController";
import { previewPhysicalPosition } from "./previewPhysicalPosition";

export { COMPACT_PREVIEW_LABEL } from "./compactPreviewProtocol";
export type CompactPreviewAnchor = {
    clientX: number; clientY: number; screenX: number; screenY: number;
    itemRect: CompactPreviewRect | null;
};
type RequestOptions = Omit<PreviewRequest, "requestId" | "privacyEpoch" | "maxWidth" | "maxHeight">;
type Session = {
    owner: object;
    request: PreviewRequest | null;
    isHovered: () => boolean;
    pointerInside: boolean;
    pinning: boolean;
    requestId: string;
    window: WebviewWindow | null;
    anchor: CompactPreviewAnchor;
    scale: number;
    origin: { x: number; y: number };
    mainSize: { width: number; height: number };
    workArea: { position: { x: number; y: number }; size: { width: number; height: number } };
    maxWidth: number; maxHeight: number;
    shown: boolean;
};
const sessions = createPreviewSessionQueue<Session>();
const pageId = crypto.randomUUID();
let preview: WebviewWindow | null = null;
let creating: Promise<WebviewWindow> | null = null;
let listeners: Promise<void> | null = null;
let requestTimer: { id: number; handle: ReturnType<typeof setTimeout> } | null = null;
const mountedWaiters = new Set<() => void>();
let leaveTimer: ReturnType<typeof setTimeout> | null = null;

let lastIgnoreBlur: boolean | null = null;
let requestedIgnoreBlur = false;
let nativeDialogBlurHolds = 0;
export const setIgnoreBlurSafe = (ignore: boolean) => {
    requestedIgnoreBlur = ignore;
    const effective = ignore || nativeDialogBlurHolds > 0;
    if (lastIgnoreBlur === effective) return;
    lastIgnoreBlur = effective;
    void invoke("set_ignore_blur", { ignore: effective }).catch(() => { lastIgnoreBlur = null; });
};
export const holdNativeDialogBlur = async (): Promise<() => void> => {
    nativeDialogBlurHolds += 1;
    try {
        await invoke("set_ignore_blur", { ignore: true });
        lastIgnoreBlur = true;
    } catch (error) {
        nativeDialogBlurHolds -= 1;
        setIgnoreBlurSafe(requestedIgnoreBlur);
        throw error;
    }
    return () => { nativeDialogBlurHolds -= 1; setIgnoreBlurSafe(requestedIgnoreBlur); };
};
const stopRequestTimer = (id?: number) => {
    if (id !== undefined && requestTimer?.id !== id) return;
    if (requestTimer !== null) clearTimeout(requestTimer.handle);
    requestTimer = null;
};
async function clearWindow() {
    const window = preview;
    if (!window) return;
    // Hiding is ordered after any in-flight show, before discarding its content.
    try { await window.hide(); }
    finally {
        await emitTo(COMPACT_PREVIEW_LABEL, "compact-preview-clear", null).catch(() => undefined);
        await emitTo(COMPACT_PREVIEW_LABEL, "privacy-view-hidden", null).catch(() => undefined);
    }
}
export function hideCompactPreviewGlobal(owner?: object): Promise<void> {
    const owned = owner === undefined || sessions.owns(owner);
    const result = sessions.cancel(clearWindow, owner);
    if (owned) {
        if (leaveTimer !== null) clearTimeout(leaveTimer);
        leaveTimer = null;
        stopRequestTimer();
        setIgnoreBlurSafe(false);
    }
    return result.catch(error => { compactPreviewLog("preview hide failed", error); });
}
export function leaveCompactPreview(owner: object): void {
    if (!sessions.owns(owner)) return;
    const id = sessions.current();
    const value = id === null ? null : sessions.get(id);
    if (!value?.shown) { void hideCompactPreviewGlobal(owner); return; }
    if (leaveTimer !== null) clearTimeout(leaveTimer);
    leaveTimer = setTimeout(() => {
        leaveTimer = null;
        if (id !== null && sessions.get(id) === value && !value.pointerInside && !value.isHovered() && !value.pinning) {
            void hideCompactPreviewGlobal(owner);
        }
    }, 500);
}
async function ensureListeners() {
    if (listeners) return listeners;
    listeners = (async () => {
        const close = () => { void hideCompactPreviewGlobal(); };
        subscribePrivacyInvalidation(close);
        window.addEventListener("scroll", close, true);
        await Promise.all([
            getCurrentWindow().listen<{ label: string }>("compact-preview-mounted", event => {
                if (event.payload?.label === COMPACT_PREVIEW_LABEL) mountedWaiters.forEach(resolve => resolve());
            }),
            getCurrentWindow().listen<PreviewLayout>("compact-preview-layout", event => { void present(event.payload); }),
            getCurrentWindow().listen<{ requestId: string }>("compact-preview-failed", event => {
                const id = sessions.current();
                if (id !== null && sessions.get(id)?.requestId === event.payload?.requestId) close();
            }),
            getCurrentWindow().listen<{ requestId: string; inside: boolean }>("compact-preview-pointer", event => {
                const id = sessions.current();
                const value = id === null ? null : sessions.get(id);
                if (!value || event.payload?.requestId !== value.requestId) return;
                value.pointerInside = event.payload.inside;
                if (value.pointerInside && leaveTimer !== null) { clearTimeout(leaveTimer); leaveTimer = null; }
                else if (!value.isHovered()) leaveCompactPreview(value.owner);
            }),
            getCurrentWindow().listen<{ requestId: string }>("compact-preview-pin", event => {
                const id = sessions.current();
                const value = id === null ? null : sessions.get(id);
                if (!value?.window || !value.request || value.pinning || event.payload?.requestId !== value.requestId) return;
                value.pinning = true;
                void value.window.innerPosition().then(position => openPinnedPreview(value.request!, position,
                    () => id !== null && sessions.get(id) === value)).then(() => {
                        if (id !== null && sessions.get(id) === value) return hideCompactPreviewGlobal(value.owner);
                    }).catch(error => {
                        value.pinning = false;
                        compactPreviewLog("pin preview failed", error);
                        void emitTo(COMPACT_PREVIEW_LABEL, "compact-preview-pin-failed", { requestId: value.requestId }).catch(() => undefined);
                        if (!value.pointerInside && !value.isHovered()) leaveCompactPreview(value.owner);
                    });
            }),
            getCurrentWindow().onMoved(close), getCurrentWindow().onResized(close),
            ...["tauri://hide", "tauri://close-requested", "tauri://destroyed"].map(name => getCurrentWindow().listen(name, close))
        ]);
    })();
    return listeners;
}
async function waitForMounted(window: WebviewWindow) {
    await new Promise<void>((resolve, reject) => {
        const finish = (error?: unknown) => {
            clearTimeout(timer); mountedWaiters.delete(ready);
            if (error) reject(error); else resolve();
        };
        const ready = () => finish();
        const timer = setTimeout(() => finish(new Error("PreviewMountTimeout")), 4000);
        mountedWaiters.add(ready);
        void emitTo(window.label, "compact-preview-probe", null).catch(finish);
    });
}
async function ensureWindow(): Promise<WebviewWindow> {
    if (preview) return preview;
    if (creating) return creating;
    creating = (async () => {
        const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
        let window = await WebviewWindow.getByLabel(COMPACT_PREVIEW_LABEL);
        if (!window) {
            window = new WebviewWindow(COMPACT_PREVIEW_LABEL, {
                url: "index.html?window=compact-preview", parent: "main",
                width: 560, height: 560, decorations: false, transparent: true,
                resizable: false, skipTaskbar: true, alwaysOnTop: true,
                visible: false, focus: false, focusable: false, shadow: false
            });
            const createdWindow = window;
            await new Promise<void>((resolve, reject) => {
                const stops: Array<() => void> = [];
                let finished = false;
                const finish = (error?: unknown) => {
                    finished = true; clearTimeout(timer); stops.forEach(stop => stop());
                    if (error) reject(error); else resolve();
                };
                const timer = setTimeout(() => finish(new Error("PreviewCreateTimeout")), 4000);
                for (const [name, handler] of [
                    ["tauri://created", () => finish()],
                    ["tauri://error", () => finish(new Error("PreviewCreateFailed"))]
                ] as const) void createdWindow.once(name, handler).then(stop => { if (finished) stop(); else stops.push(stop); }, finish);
            });
        }
        const owned = window;
        await owned.hide();
        await owned.setIgnoreCursorEvents(false);
        await owned.once("tauri://destroyed", () => {
            if (preview === owned) { preview = null; void hideCompactPreviewGlobal(); }
        });
        preview = owned;
        return owned;
    })().finally(() => { creating = null; });
    return creating;
}
async function present(layout: PreviewLayout) {
    const id = sessions.current();
    if (id === null) return;
    const session = sessions.get(id);
    if (!session || layout?.requestId !== session.requestId || !session.window
        || !Number.isFinite(layout.width) || !Number.isFinite(layout.height) || layout.width < 1 || layout.height < 1) return;
    const width = Math.min(layout.width, session.maxWidth);
    const height = Math.min(layout.height, session.maxHeight);
    const scale = session.scale;
    const size = new PhysicalSize(Math.ceil(width * scale), Math.ceil(height * scale));
    const anchor = session.anchor;
    const rect = anchor.itemRect;
    const target = pickPreviewPosition({
        anchorX: session.origin.x + anchor.clientX * scale, anchorY: session.origin.y + anchor.clientY * scale,
        anchorRect: rect && { left: session.origin.x + rect.left * scale, right: session.origin.x + rect.right * scale,
            top: session.origin.y + rect.top * scale, bottom: session.origin.y + rect.bottom * scale },
        widthPx: size.width, heightPx: size.height, monitorPos: session.workArea.position, monitorSize: session.workArea.size,
        margin: Math.round(10 * scale), offset: Math.round(12 * scale),
        avoidRect: { left: session.origin.x, top: session.origin.y, right: session.origin.x + session.mainSize.width,
            bottom: session.origin.y + session.mainSize.height }
    });
    try {
        await sessions.run(id, [
            async value => { await value.window!.setSize(size); },
            async value => {
                const position = previewPhysicalPosition(target);
                await value.window!.setPosition(new PhysicalPosition(position.x, position.y));
            },
            async value => {
                if (value.shown) return;
                await value.window!.setAlwaysOnTop(true);
            },
            async value => {
                if (value.shown) return;
                if (!(await getCurrentWindow().isVisible()) || !sessions.get(id)) return;
                setIgnoreBlurSafe(true);
                await invoke("show_compact_preview_without_activation");
                if (!sessions.get(id)) return;
                await emitTo(COMPACT_PREVIEW_LABEL, "privacy-view-shown", null);
                if (!sessions.get(id)) return;
                value.shown = true;
                stopRequestTimer(id);
            }
        ]);
    } catch (error) {
        compactPreviewLog("preview presentation failed", error);
        if (sessions.current() === id) await hideCompactPreviewGlobal();
    }
}
export async function showCompactPreview(owner: object, anchor: CompactPreviewAnchor, options: RequestOptions,
    stillHovered: () => boolean, sourceIsCurrent: () => boolean = stillHovered) {
    const access = beginPrivacyAccess();
    if (!access || !stillHovered()) return;
    stopRequestTimer();
    const value: Session = { owner, request: null, isHovered: stillHovered, pointerInside: false, pinning: false,
        requestId: "", window: null, anchor, scale: 1, origin: { x: 0, y: 0 },
        mainSize: { width: 0, height: 0 }, workArea: { position: { x: 0, y: 0 }, size: { width: 1280, height: 720 } },
        maxWidth: 560, maxHeight: 560, shown: false };
    const id = sessions.begin(owner, value, () => access.isCurrent() && sourceIsCurrent());
    value.requestId = `${pageId}:${id}`;
    requestTimer = { id, handle: setTimeout(() => { if (sessions.current() === id) void hideCompactPreviewGlobal(); }, 6000) };
    try {
        await ensureListeners();
        const window = await ensureWindow();
        await waitForMounted(window);
        if (!sessions.get(id)) return;
        const main = getCurrentWindow();
        const [scale, origin, mainSize, monitor] = await Promise.all([main.scaleFactor(), main.innerPosition(), main.innerSize(), currentMonitor()]);
        if (!sessions.get(id)) return;
        value.window = window; value.scale = scale; value.origin = origin; value.mainSize = mainSize;
        if (monitor) value.workArea = monitor.workArea;
        value.maxWidth = Math.max(1, Math.min(560, Math.floor(value.workArea.size.width / scale - 20)));
        value.maxHeight = Math.max(1, Math.min(560, Math.floor(value.workArea.size.height / scale - 20)));
        const request: PreviewRequest = { ...options, requestId: value.requestId, privacyEpoch: access.epoch,
            maxWidth: value.maxWidth, maxHeight: value.maxHeight };
        value.request = request;
        await sessions.run(id, [
            async () => { setIgnoreBlurSafe(false); await clearWindow(); },
            async () => { await window.setSize(new PhysicalSize(Math.ceil(value.maxWidth * scale), Math.ceil(value.maxHeight * scale))); },
            async () => { await emitTo(window.label, "compact-preview-update", request); }
        ]);
    } catch (error) {
        compactPreviewLog("preview request failed", error);
        if (sessions.current() === id) await hideCompactPreviewGlobal();
    }
}
