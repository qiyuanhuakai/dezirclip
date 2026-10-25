import { useEffect, useLayoutEffect, useMemo, useState, useRef } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
    AppWindow,
    Clock
} from "lucide-react";
import HtmlContent from "../../../shared/components/HtmlContent";
import { getConciseTime } from "../../../shared/lib/utils";
import type { PreviewRequest } from "./compactPreviewProtocol";
import { isRamImageSource, isSensitiveEntry } from "../../../shared/lib/privacyRuntime";
import { getRichTextSnapshotDataUrl } from "../../../shared/lib/richTextSnapshot";
import { getRichPreviewData } from "../../../shared/lib/richPreviewState";
import { applyModeClass, applyThemeClass } from "../../../shared/lib/themeRuntime";
import { useBackendAppearance } from "../../../shared/hooks/useBackendAppearance";
import { seekVideoPreviewFrame } from "../../../shared/lib/videoPreview";
import { getContentTypeIcon } from "../../../shared/lib/contentTypeIcon";
import { usePrivacySession } from "../../../shared/hooks/usePrivacySession";
import { prepareOwnedPreview, invokePrivacyBound, subscribePrivacyInvalidation } from "../../../shared/lib/privacySession";
import { clearRichTextSnapshots } from "../../../shared/lib/richTextSnapshot";
import type { ClipboardEntry } from "../../../shared/types";

type PreviewPayload = PreviewRequest & {
    protectedContent: boolean;
    contentType: string;
    content: string;
    preview?: string;
    htmlContent?: string;
    sourceApp?: string;
    timestamp?: number;
};

import { compactPreviewLog, richPreviewFailureLog } from "../../../shared/lib/compactPreviewLog";

const getIcon = (type: string) => getContentTypeIcon(type);

const applyTheme = (payload: PreviewPayload) => {
    const theme = payload.theme || "mica";
    const colorMode = payload.colorMode || "light";

    const root = document.documentElement;
    const body = document.body;

    applyThemeClass(root, body, theme);
    applyModeClass(root, body, colorMode === "dark" ? "dark" : "light");

    body.classList.add("compact-preview");

    if (payload.clipboardItemFontSize) {
        root.style.setProperty("--clipboard-item-font-size", `${payload.clipboardItemFontSize}px`);
    }
    if (payload.clipboardTagFontSize) {
        root.style.setProperty("--clipboard-tag-font-size", `${payload.clipboardTagFontSize}px`);
    }

    compactPreviewLog("theme applied", {
        theme,
        colorMode,
        itemFontSize: payload.clipboardItemFontSize,
        tagFontSize: payload.clipboardTagFontSize
    });
};

const CompactPreviewWindow = () => {
    const privacyState = usePrivacySession();
    const [storedPayload, setPayload] = useState<PreviewPayload | null>(null);
    const [previewRequest, setPreviewRequest] = useState<{ value: PreviewRequest; sequence: number } | null>(null);
    const payload = privacyState.phase === "ready" && storedPayload?.privacyEpoch === privacyState.snapshot?.privacy_epoch ? storedPayload : null;
    const payloadSequence = useRef(0);
    const [previewListenerReady, setPreviewListenerReady] = useState(false);
    useEffect(() => subscribePrivacyInvalidation(() => {
        payloadSequence.current += 1;
        setPreviewRequest(null);
        setPayload(null);
        clearRichTextSnapshots();
    }), []);
    useEffect(() => {
        if (!previewRequest) return;
        const { value: request, sequence } = previewRequest;
        let disposed = false;
        void (async () => {
            const access = await prepareOwnedPreview(request.privacyEpoch,
                () => !disposed && sequence === payloadSequence.current, request.allowSensitive);
            if (disposed || sequence !== payloadSequence.current || !access.isCurrent()) return;
            const entry = await invokePrivacyBound<ClipboardEntry>(request.allowSensitive ? "get_privacy_entry" : "get_history_resource_preview",
                { id: request.id, expectedHostInstanceId: request.expectedHostInstanceId, expectedEpoch: request.privacyEpoch, maxDimension: 560 });
            if (disposed || sequence !== payloadSequence.current || !access.isCurrent()) return;
            const next: PreviewPayload = { ...request, contentType: entry.content_type, content: entry.content,
                protectedContent: isSensitiveEntry(entry), preview: entry.preview, htmlContent: entry.html_content,
                sourceApp: entry.source_app, timestamp: entry.timestamp };
            applyTheme(next);
            setPayload(next);
        })().catch(() => {
            if (disposed || sequence !== payloadSequence.current) return;
            setPayload(null);
            void emitTo("main", "compact-preview-failed", { requestId: request.requestId }).catch(() => undefined);
        });
        return () => { disposed = true; };
    }, [previewRequest]);
    useEffect(() => {
        if (previewListenerReady) void emitTo("main", "compact-preview-mounted", true).catch(() => undefined);
    }, [previewListenerReady]);
    // The hover card can be opened before the main window has ever rendered, so
    // the appearance comes from the backend rather than from local state.
    useBackendAppearance();
    const [snapshotFailed, setSnapshotFailed] = useState(false);
    const [richImageFallbackFailed, setRichImageFallbackFailed] = useState(false);
    const richSnapshotImgRef = useRef<HTMLImageElement | null>(null);
    const richSnapshotFallbackTimerRef = useRef<number | null>(null);
    const containerRef = useRef<HTMLDivElement | null>(null);
    const metaRef = useRef<HTMLDivElement | null>(null);
    const contentRef = useRef<HTMLDivElement | null>(null);
    const dividerRef = useRef<HTMLDivElement | null>(null);
    const requestResizeRef = useRef<(() => void) | null>(null);
    const previewBoundsRef = useRef({ width: 560, height: 560, mediaWidth: 520, mediaHeight: 360 });
    const lastSentSizeRef = useRef<{ requestId: string; width: number; height: number } | null>(null);
    const richPreviewData = useMemo(() => getRichPreviewData(payload), [payload?.contentType, payload?.htmlContent]);
    const richImageFallbackSrc = richPreviewData.imageSrc;
    const richTextSnapshotHtml = richPreviewData.cleanHtml || payload?.htmlContent || "";
    const richTextSnapshotSrc = useMemo(() => {
        if (!payload || payload.contentType !== "rich_text" || !payload.htmlContent) return null;
        if (!payload.richTextSnapshotPreview) return null;
        return getRichTextSnapshotDataUrl(richTextSnapshotHtml, {
            width: 560,
            maxHeight: 1200
        });
    }, [payload?.contentType, payload?.htmlContent, payload?.richTextSnapshotPreview, richTextSnapshotHtml]);
    const effectiveRichTextSnapshotSrc = snapshotFailed ? null : richTextSnapshotSrc;
    const effectiveRichImageFallbackSrc = richImageFallbackFailed ? null : richImageFallbackSrc;
    const useSnapshotPreviewImage = !effectiveRichImageFallbackSrc && !!effectiveRichTextSnapshotSrc;

    useEffect(() => {
        document.body.classList.add("compact-preview");
        compactPreviewLog("window mounted", { label: getCurrentWindow().label });
        let disposed = false;
        const unlisten = listen<PreviewRequest>("compact-preview-update", (event) => {
            const request = event.payload;
            if (!request || typeof request.requestId !== "string" || !Number.isSafeInteger(request.id) || request.id === 0
                || !Number.isSafeInteger(request.privacyEpoch) || request.privacyEpoch < 0
                || !Number.isFinite(request.maxWidth) || request.maxWidth <= 0 || request.maxWidth > 560
                || !Number.isFinite(request.maxHeight) || request.maxHeight <= 0 || request.maxHeight > 560) return;
            const sequence = ++payloadSequence.current;
            lastSentSizeRef.current = null;
            const mediaWidth = Math.min(520, request.maxWidth);
            const mediaHeight = Math.max(1, Math.min(360, request.maxHeight - 80));
            previewBoundsRef.current = { width: request.maxWidth, height: request.maxHeight, mediaWidth, mediaHeight };
            const root = document.documentElement;
            root.style.setProperty("--preview-max-width", `${request.maxWidth}px`);
            root.style.setProperty("--preview-max-height", `${request.maxHeight}px`);
            root.style.setProperty("--preview-media-max-width", `${mediaWidth}px`);
            root.style.setProperty("--preview-media-max-height", `${mediaHeight}px`);
            root.style.setProperty("--preview-min-width", `${Math.min(320, request.maxWidth)}px`);
            if (!disposed) { setPayload(null); setPreviewRequest({ value: request, sequence }); }
        });
        const clear = () => {
            payloadSequence.current += 1;
            setPreviewRequest(null);
            setPayload(null);
            lastSentSizeRef.current = null;
            clearRichTextSnapshots();
        };
        const unlistenClear = listen("compact-preview-clear", clear);
        const unlistenProbe = listen("compact-preview-probe", () => {
            void Promise.all([unlisten, unlistenClear]).then(() => {
                if (!disposed) return emitTo("main", "compact-preview-mounted", true);
            }).catch(() => undefined);
        });
        const subscriptions = [unlisten, unlistenClear, unlistenProbe];
        void Promise.all(subscriptions).then(() => { if (!disposed) setPreviewListenerReady(true); }).catch(() => undefined);
        return () => {
            disposed = true;
            payloadSequence.current += 1;
            subscriptions.forEach(subscription => { void subscription.then(stop => stop(), () => undefined); });
        };
    }, []);

    useLayoutEffect(() => {
        if (!payload) { requestResizeRef.current = null; return; }
        let disposed = false;
        const updateSize = () => {
            if (disposed) return;
            const container = containerRef.current;
            if (!container) return;
            // Decode media before the first presentation. Hidden WebViews may suspend animation frames.
            if (Array.from(container.querySelectorAll("img")).some(image => !image.complete)) return;
            if (Array.from(container.querySelectorAll("video")).some(video => video.readyState < 1 && !video.error)) return;
            const bounds = previewBoundsRef.current;
            const width = Math.min(Math.max(Math.ceil(container.scrollWidth), Math.min(320, bounds.width)), bounds.width);
            const height = Math.min(Math.max(Math.ceil(container.offsetHeight), 80), bounds.height);
            const last = lastSentSizeRef.current;
            if (last?.requestId === payload.requestId && last.width === width && last.height === height) return;
            lastSentSizeRef.current = { requestId: payload.requestId, width, height };
            void emitTo("main", "compact-preview-layout", { requestId: payload.requestId, width, height }).catch(() => undefined);
        };
        requestResizeRef.current = updateSize;
        updateSize();
        const observer = new ResizeObserver(updateSize);
        if (containerRef.current) observer.observe(containerRef.current);
        if (contentRef.current) observer.observe(contentRef.current);
        void document.fonts.ready.then(updateSize);
        return () => {
            disposed = true;
            requestResizeRef.current = null;
            observer.disconnect();
        };
    }, [payload, effectiveRichImageFallbackSrc, effectiveRichTextSnapshotSrc]);

    useEffect(() => {
        setSnapshotFailed(false);
        setRichImageFallbackFailed(false);
    }, [payload?.content, payload?.htmlContent, payload?.richTextSnapshotPreview]);

    useEffect(() => {
        if (richSnapshotFallbackTimerRef.current) {
            window.clearTimeout(richSnapshotFallbackTimerRef.current);
            richSnapshotFallbackTimerRef.current = null;
        }
        if (!useSnapshotPreviewImage) return;

        // Safety net: in some environments broken SVG data urls don't always emit onError.
        richSnapshotFallbackTimerRef.current = window.setTimeout(() => {
            const img = richSnapshotImgRef.current;
            if (!img || !img.complete || img.naturalWidth <= 0 || img.naturalHeight <= 0) {
                richPreviewFailureLog("snapshot image timeout -> fallback to html", {
                    hasImageElement: !!img,
                    complete: img?.complete ?? false,
                    naturalWidth: img?.naturalWidth ?? 0,
                    naturalHeight: img?.naturalHeight ?? 0,
                    sourceApp: payload?.sourceApp || ""
                });
                setSnapshotFailed(true);
            }
        }, 700);

        return () => {
            if (richSnapshotFallbackTimerRef.current) {
                window.clearTimeout(richSnapshotFallbackTimerRef.current);
                richSnapshotFallbackTimerRef.current = null;
            }
        };
    }, [useSnapshotPreviewImage, effectiveRichTextSnapshotSrc, payload?.content, payload?.htmlContent]);

    const content = useMemo(() => {
        if (!payload) return null;
        if (payload.contentType === "image") {
            const src = isRamImageSource(payload.content) ? payload.content : undefined;
            return (
                <img
                    src={src}
                    alt="preview"
                    onError={() => requestResizeRef.current?.()}
                    onLoad={() => {
                        compactPreviewLog("image loaded, request resize");
                        requestResizeRef.current?.();
                    }}
                    style={{
                        width: "auto",
                        height: "auto",
                        borderRadius: "4px"
                    }}
                />
            );
        }
        if (payload.contentType === "video") {
            const src = /^data:video\/(mp4|webm);base64,[A-Za-z0-9+/=]+$/.test(payload.content) ? payload.content : undefined;
            return (
                <video
                    src={src}
                    preload="metadata"
                    muted
                    playsInline
                    onError={() => requestResizeRef.current?.()}
                    onLoadedMetadata={(e) => {
                        compactPreviewLog("video metadata loaded", {
                            duration: e.currentTarget.duration
                        });
                        seekVideoPreviewFrame(e.currentTarget);
                        requestResizeRef.current?.();
                    }}
                    style={{
                        width: "auto",
                        height: "auto",
                        borderRadius: "4px",
                        background: "#000"
                    }}
                />
            );
        }
        if (payload.contentType === "rich_text" && payload.htmlContent) {
            if (effectiveRichImageFallbackSrc) {
                compactPreviewLog("render rich_text as fallback image");
                return (
                    <img
                        src={effectiveRichImageFallbackSrc}
                        alt="rich text preview"
                        onLoad={() => {
                            compactPreviewLog("rich fallback image loaded, request resize");
                            requestResizeRef.current?.();
                        }}
                        onError={() => {
                            richPreviewFailureLog("fallback image load error -> switch to snapshot", {
                                srcLength: (effectiveRichImageFallbackSrc || "").length,
                                sourceApp: payload.sourceApp || ""
                            });
                            setRichImageFallbackFailed(true);
                        }}
                        style={{
                            width: "auto",
                            height: "auto",
                            borderRadius: "4px"
                        }}
                    />
                );
            }
            if (effectiveRichTextSnapshotSrc) {
                compactPreviewLog("render rich_text as html snapshot image");
                return (
                    <img
                        ref={richSnapshotImgRef}
                        src={effectiveRichTextSnapshotSrc}
                        alt="rich text snapshot preview"
                        onLoad={() => {
                            if (richSnapshotFallbackTimerRef.current) {
                                window.clearTimeout(richSnapshotFallbackTimerRef.current);
                                richSnapshotFallbackTimerRef.current = null;
                            }
                            compactPreviewLog("rich snapshot image loaded, request resize");
                            requestResizeRef.current?.();
                        }}
                        onError={() => {
                            if (richSnapshotFallbackTimerRef.current) {
                                window.clearTimeout(richSnapshotFallbackTimerRef.current);
                                richSnapshotFallbackTimerRef.current = null;
                            }
                            richPreviewFailureLog("snapshot image load error -> fallback to html", {
                                srcLength: (effectiveRichTextSnapshotSrc || "").length,
                                sourceApp: payload.sourceApp || ""
                            });
                            setSnapshotFailed(true);
                        }}
                        style={{
                            width: "100%",
                            maxWidth: "100%",
                            height: "auto",
                            display: "block",
                            borderRadius: "4px"
                        }}
                    />
                );
            }
            return (
                <HtmlContent
                    className="rich-text-preview"
                    htmlContent={richPreviewData.cleanHtml || payload.htmlContent}
                    fallbackText={payload.preview || payload.content}
                    preview={false}
                    style={{
                        // Keep a single scrollbar on .popover-content to avoid nested scrollbars.
                        maxHeight: "none",
                        overflow: "visible",
                        fontSize: "var(--clipboard-item-font-size)",
                        lineHeight: "1.5"
                    }}
                />
            );
        }
        return payload.content || payload.preview || "";
    }, [payload, effectiveRichImageFallbackSrc, effectiveRichTextSnapshotSrc, richPreviewData.cleanHtml]);

    if (!payload) return null;
    return (
        <div className="compact-preview-root">
            <div
                ref={containerRef}
                className={`compact-popover-portal compact-preview-window ${payload?.theme || ""} ${payload?.contentType === "image" ? "compact-preview-image" : ""} ${payload?.contentType === "image" || payload?.contentType === "video" || !!effectiveRichImageFallbackSrc ? "compact-preview-media" : ""} ${payload?.colorMode === "dark" ? "dark-mode" : ""}`}
            >
                <div ref={metaRef} className="popover-meta">
                    <div className="meta-row">
                        {getIcon(payload?.contentType || "text")}
                        <span>{payload?.contentType || "text"}</span>
                    </div>
                    <div className="meta-dot">•</div>
                    <div className="meta-row">
                        <AppWindow size={14} />
                        <span>{payload?.sourceApp || "Unknown"}</span>
                    </div>
                    <div className="meta-dot">•</div>
                    <div className="meta-row">
                        <Clock size={14} />
                        <span>
                            {payload?.timestamp && payload?.language
                                ? getConciseTime(payload.timestamp, payload.language)
                                : "-"}
                        </span>
                    </div>
                </div>
                <div ref={dividerRef} className="popover-divider" />
                <div ref={contentRef} className="popover-content" data-privacy-protected={payload?.protectedContent ? "true" : undefined}
                    onDragStart={event => { if (payload?.protectedContent) event.preventDefault(); }}>{content}</div>
            </div>
        </div>
    );
};

export default CompactPreviewWindow;
