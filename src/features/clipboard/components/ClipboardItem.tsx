import { useRef, useEffect, useState, useMemo, useCallback, memo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
    Pin,
    PinOff,
    Eye,
    EyeOff,
    ExternalLink,
    Tag,
    X,
    FileText,
    File,
    Plus,
    Video,
    FileArchive,
    Music,
    FileCode,
    Cpu,
    Files,
    ImageOff,
    FileQuestion,
    GripVertical,
    Edit2
} from "lucide-react";
import { motion } from "framer-motion";
import type { ClipboardItemProps } from "../types";
import type { ClipboardEntry } from "../../../shared/types";
import { getConciseTime, getTagColor } from "../../../shared/lib/utils";
import HtmlContent from "../../../shared/components/HtmlContent";
import { isSensitiveEntry } from "../../../shared/lib/privacyRuntime";
import { useImagePreview } from "../../../shared/hooks/useImagePreview";
import { extractRichImageFallback, resolveRichImageSrc } from "../../../shared/lib/richPreview";
import { getSourceAppIcon, peekSourceAppIcon } from "../../../shared/lib/sourceAppIcon";
import { seekVideoPreviewFrame } from "../../../shared/lib/videoPreview";
import { getContentTypeIcon } from "../../../shared/lib/contentTypeIcon";
import { createFrameThrottle } from "../../../shared/lib/frameThrottle";
import { sameListById } from "../../../shared/lib/sameListById";
import { ItemContextMenu } from "./ItemContextMenu";
import type { TransformKindDto } from "./ItemContextMenu";
import type { CompactPreviewRect } from "./compactPreviewPosition";
import { COMPACT_PREVIEW_LABEL, hideCompactPreviewGlobal, showCompactPreview as requestCompactPreview,
    holdNativeDialogBlur, setIgnoreBlurSafe, type CompactPreviewAnchor } from "./compactPreviewController";
export { hideCompactPreviewGlobal } from "./compactPreviewController";
import { resolveContextMenuPoint } from "./contextMenuPosition";
import { beginPrivacyAccess, getPrivacyViewState, invokePrivacyBound, refreshPrivacyAfterConfirmation, submitPrivacyFeedback, subscribePrivacyInvalidation } from "../../../shared/lib/privacySession";
import type { PrivacyFeedbackAction } from "../../../shared/lib/privacySession";
import { ask } from "@tauri-apps/plugin-dialog";
import { capturePrivacyMetadataIntent } from "../../../shared/lib/privacyMetadataIntent";
import type { PrivacyMetadataIntent } from "../../../shared/lib/privacyMetadataIntent";

const CONTEXT_MENU_OPEN_EVENT = "dezirclip:context-menu-open";
import { compactPreviewLog, richPreviewFailureLog } from "../../../shared/lib/compactPreviewLog";

const toCompactPreviewRect = (rect: DOMRect): CompactPreviewRect => ({
    left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom
});

// `list_transform_kinds` answers from a compile-time table, so one fetch for
// the whole app is enough. Every mounted row used to ask for its own copy.
let transformKindsRequest: Promise<TransformKindDto[]> | null = null;
// The resolved list is kept next to the request. A row that mounts after the
// first row has already fetched starts with the answer, instead of holding an
// empty list until a promise it did not need hands it the same data back.
let transformKindsCache: TransformKindDto[] | null = null;

const fetchTransformKinds = (): Promise<TransformKindDto[]> => {
    if (!transformKindsRequest) {
        transformKindsRequest = invoke<TransformKindDto[]>("list_transform_kinds")
            .then((kinds) => {
                transformKindsCache = Array.isArray(kinds) ? kinds : [];
                return transformKindsCache;
            })
            .catch((err) => {
                transformKindsRequest = null;
                throw err;
            });
    }
    return transformKindsRequest;
};

type OcrCompletePayload = { item_id: number; text: string; status: string };

// One backend subscription shared by every mounted row. Each row only cares
// about its own entry, so N listeners were N register/unregister round trips
// per scroll with no behavioural difference between one listener and N.
//
// The registration outlives the rows. It used to be dropped as soon as the last
// row unsubscribed, but a capture that prepends an entry shifts every row one
// slot, so each row tore its subscription down and built a new one in the same
// commit. React runs every teardown before any setup, so the shared set went
// empty mid-commit and the next subscriber registered the listener again: one
// `plugin:event|listen` plus one `plugin:event|unlisten` per captured entry, to
// no effect. One registration for the lifetime of the page costs nothing --
// there is no per-row payload left to drop -- and a failed registration still
// clears itself so a later subscriber can retry.
const ocrSubscribers = new Set<{
    matches: (itemId: number) => boolean;
    handle: (p: OcrCompletePayload) => void;
}>();
let ocrUnlisten: Promise<() => void> | null = null;

const subscribeOcrComplete = (
    matches: (itemId: number) => boolean,
    handle: (payload: OcrCompletePayload) => void
): (() => void) => {
    const entry = { matches, handle };
    ocrSubscribers.add(entry);
    if (!ocrUnlisten) {
        ocrUnlisten = listen<OcrCompletePayload>("ocr:complete", (event) => {
            for (const sub of ocrSubscribers) {
                if (sub.matches(event.payload.item_id)) sub.handle(event.payload);
            }
        }).catch(() => {
            // Let a later subscriber retry the registration.
            ocrUnlisten = null;
            return () => {};
        });
    }
    return () => {
        ocrSubscribers.delete(entry);
    };
};

const getIcon = (type: string) => getContentTypeIcon(type);

const renderSourceAppIcon = (iconSrc: string | null, contentType: string, sourceApp: string) => {
    if (!iconSrc) {
        return getIcon(contentType);
    }

    return (
        <img
            src={iconSrc}
            alt={`${sourceApp} icon`}
            className="source-app-icon"
            loading="lazy"
        />
    );
};

const getFileIcon = (filePath: string) => {
    const ext = filePath.split('.').pop()?.toLowerCase();
    switch (ext) {
        case 'zip':
        case 'rar':
        case '7z':
        case 'tar':
        case 'gz':
            return <FileArchive size={20} />;
        case 'mp3':
        case 'wav':
        case 'flac':
        case 'm4a':
            return <Music size={20} />;
        case 'exe':
        case 'msi':
        case 'bat':
        case 'sh':
            return <Cpu size={20} />;
        case 'pdf':
        case 'doc':
        case 'docx':
        case 'ppt':
        case 'pptx':
        case 'xls':
        case 'xlsx':
            return <FileText size={20} />;
        case 'js':
        case 'ts':
        case 'tsx':
        case 'jsx':
        case 'py':
        case 'rs':
        case 'c':
        case 'cpp':
        case 'go':
        case 'java':
        case 'html':
        case 'css':
        case 'json':
            return <FileCode size={20} />;
        default:
            return <File size={20} />;
    }
};

const ClipboardItem = ({
    item,
    isSelected,
    windowPinned,
    isSensitiveHidden,
    isRevealed,
    showSensitiveVisibilityControl = true,
    isEditingTags,
    tagInput,
    theme,
    language,
    t,
    onSelect,
    onCopy,
    onToggleReveal,
    onOpen,
    onTogglePin,
    onDelete,
    onToggleTagEditor,
    onTagInput,
    onTagAdd,
    onTagDelete,
    tagColors,
    richTextSnapshotPreview = false,
    dragControls,
    id,
    compactMode,
    className,
    disableLayout,
    onQRCode,
    onTransformItem,
    onTransformItemError,
    onTransformItemSuccess,
    loadFullItem,
    onEdit,
}: ClipboardItemProps & {
    showSensitiveVisibilityControl?: boolean;
    compactMode?: boolean;
    className?: string;
    onQRCode?: () => void;
    onTransformItem?: (kind: string) => void;
    onTransformItemError?: (kind: string, message: string) => void;
    onTransformItemSuccess?: (kind: string) => void;
    loadFullItem?: () => Promise<ClipboardEntry>;
    onEdit?: () => void;
}) => {
    const tagInputRef = useRef<HTMLInputElement>(null);
    const hiddenRef = useRef(isSensitiveHidden);
    hiddenRef.current = isSensitiveHidden;
    const contentIdentity = useRef({ item, hidden: isSensitiveHidden, generation: 0 });
    if (contentIdentity.current.item !== item || contentIdentity.current.hidden !== isSensitiveHidden) {
        contentIdentity.current = { item, hidden: isSensitiveHidden, generation: contentIdentity.current.generation + 1 };
    }
    const [localTagInput, setLocalTagInput] = useState(tagInput);
    const [snapshotFailed, setSnapshotFailed] = useState(false);
    const [richImageFallbackFailed, setRichImageFallbackFailed] = useState(false);
    const [richTextSnapshotSrc, setRichTextSnapshotSrc] = useState<string | null>(null);
    const [sourceAppIcon, setSourceAppIcon] = useState<string | null>(() => peekSourceAppIcon(item.source_app_path) ?? null);
    const [contextMenuState, setContextMenuState] = useState<{ x: number; y: number } | null>(null);
    const [privacyFeedbackPending, setPrivacyFeedbackPending] = useState(false);
    const [metadataActionPending, setMetadataActionPending] = useState(false);
    const metadataActionRunning = useRef(false);
    const metadataMounted = useRef(true);
    useEffect(() => { metadataMounted.current = true; return () => { metadataMounted.current = false; }; }, []);
    const runMetadataAction = useCallback(async (action: (intent: PrivacyMetadataIntent) => void | Promise<void>) => {
        if (metadataActionRunning.current) return;
        const intent = capturePrivacyMetadataIntent(getPrivacyViewState());
        if (!intent) { onTransformItemError?.("privacy_metadata", t("privacy_action_failed")); return; }
        metadataActionRunning.current = true;
        setMetadataActionPending(true);
        let release: (() => void) | undefined;
        try {
            release = await holdNativeDialogBlur();
            if (!metadataMounted.current) return;
            await action(intent);
        } catch {
            if (metadataMounted.current) onTransformItemError?.("privacy_metadata", t("privacy_action_failed"));
        } finally {
            release?.();
            metadataActionRunning.current = false;
            if (metadataMounted.current) setMetadataActionPending(false);
        }
    }, [onTransformItemError, t]);
    const [transformKinds, setTransformKinds] = useState<TransformKindDto[]>(
        () => transformKindsCache ?? []
    );
    const [ocrTextValue, setOcrText] = useState<string | null>(item.ocr_text ?? null);
    const [ocrStatusValue, setOcrStatus] = useState<string | null>(item.ocr_status ?? null);
    const [ocrSource, setOcrSource] = useState<ClipboardEntry | null>(item);
    const ocrText = !isSensitiveHidden && ocrSource === item ? ocrTextValue : null;
    const ocrStatus = !isSensitiveHidden && ocrSource === item ? ocrStatusValue : null;
    const [ocrTextExpanded, setOcrTextExpanded] = useState(false);
    useEffect(() => subscribePrivacyInvalidation(() => {
        contentIdentity.current.generation += 1;
        setOcrSource(null);
        setOcrText(null);
        setOcrStatus(null);
        setOcrTextExpanded(false);
        setRichTextSnapshotSrc(null);
        setContextMenuState(null);
    }), []);
    const imagePreview = useImagePreview(
        item.id, item.content, item.timestamp,
        item.content_type === "image" && !isSensitiveHidden && !(item.is_external && item.file_preview_exists === false),
        item.host_instance_id
    );
    const isComposing = useRef(false);
    const richSnapshotImgRef = useRef<HTMLImageElement | null>(null);
    const richSnapshotFallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const hoverAnchorRef = useRef<CompactPreviewAnchor | null>(null);
    // One measurement per frame at most, applied inside the frame callback so
    // the layout is read after the frame's own writes rather than forcing one
    // on every pointer event.
    const hoverAnchorThrottle = useMemo(
        () =>
            createFrameThrottle<{
                clientX: number;
                clientY: number;
                screenX: number;
                screenY: number;
                element: HTMLElement;
            }>(
                ({ clientX, clientY, screenX, screenY, element }) => {
                    if (!element.isConnected) return;
                    hoverAnchorRef.current = {
                        clientX,
                        clientY,
                        screenX,
                        screenY,
                        itemRect: toCompactPreviewRect(element.getBoundingClientRect())
                    };
                },
                (cb) => requestAnimationFrame(cb),
                (handle) => cancelAnimationFrame(handle)
            ),
        []
    );
    const windowLabelRef = useRef<string>(getCurrentWindow().label);
    const richTextFallback = useMemo(() => {
        if (item.content_type !== "rich_text" || !item.html_content) return null;
        const { cleanHtml, imagePayload } = extractRichImageFallback(item.html_content);
        return {
            cleanHtml: cleanHtml || item.html_content,
            imageSrc: resolveRichImageSrc(imagePayload)
        };
    }, [item.content_type, item.html_content]);
    const richTextCleanHtml = richTextFallback?.cleanHtml || item.html_content || "";
    const richTextSnapshotDisplayMaxHeight = compactMode ? 40 : 64;
    const richTextSnapshotRenderMaxHeight = compactMode ? 100 : 200;
    const snapshotPreviewEnabled = !!richTextSnapshotPreview;
    const useRichImageFallback = snapshotPreviewEnabled && !richImageFallbackFailed && !!richTextFallback?.imageSrc;
    const shouldGenerateSnapshot =
        snapshotPreviewEnabled &&
        !snapshotFailed &&
        item.content_type === "rich_text" &&
        !!item.html_content &&
        !!richTextCleanHtml &&
        !useRichImageFallback;
    useEffect(() => {
        if (!shouldGenerateSnapshot) {
            setRichTextSnapshotSrc(null);
            return;
        }
        let cancelled = false;
        import("../../../shared/lib/richTextSnapshot")
            .then(({ getRichTextSnapshotDataUrl }) => {
                if (cancelled) return;
                const snapshot = getRichTextSnapshotDataUrl(richTextCleanHtml, {
                    width: compactMode ? 360 : 560,
                    maxHeight: richTextSnapshotRenderMaxHeight
                });
                if (!cancelled) {
                    setRichTextSnapshotSrc(snapshot);
                }
            })
            .catch(() => {
                if (!cancelled) {
                    setRichTextSnapshotSrc(null);
                }
            });
        return () => {
            cancelled = true;
        };
    }, [shouldGenerateSnapshot, richTextCleanHtml, compactMode, richTextSnapshotRenderMaxHeight]);
    const effectiveRichTextSnapshotSrc = shouldGenerateSnapshot ? richTextSnapshotSrc : null;
    const richTextPreviewSrc = useRichImageFallback
        ? (richTextFallback?.imageSrc || null)
        : effectiveRichTextSnapshotSrc;
    const useSnapshotPreviewImage = shouldGenerateSnapshot && !!effectiveRichTextSnapshotSrc;

    useEffect(() => {
        let cancelled = false;
        const sourceAppPath = item.source_app_path?.trim();
        const cachedIcon = peekSourceAppIcon(sourceAppPath);

        if (cachedIcon !== undefined) {
            setSourceAppIcon(cachedIcon ?? null);
            return () => {
                cancelled = true;
            };
        }

        setSourceAppIcon(null);
        if (!sourceAppPath) {
            return () => {
                cancelled = true;
            };
        }

        getSourceAppIcon(sourceAppPath).then((icon) => {
            if (!cancelled) {
                setSourceAppIcon(icon);
            }
        });

        return () => {
            cancelled = true;
        };
    }, [item.source_app_path]);

    const previewOwner = useRef({});
    const hideCompactPreview = () => hideCompactPreviewGlobal(previewOwner.current);
    const showCompactPreview = (anchor: CompactPreviewAnchor) => {
        const generation = contentIdentity.current.generation;
        const rootStyle = getComputedStyle(document.documentElement);
        const fontSize = (name: string) => {
            const value = parseInt(rootStyle.getPropertyValue(name));
            return Number.isFinite(value) ? value : undefined;
        };
        return requestCompactPreview(previewOwner.current, anchor, {
            id: item.id, expectedHostInstanceId: item.host_instance_id,
            allowSensitive: isSensitiveEntry(item) && isRevealed,
            language, theme, richTextSnapshotPreview,
            colorMode: document.documentElement.classList.contains("dark-mode") ? "dark" : "light",
            clipboardItemFontSize: fontSize("--clipboard-item-font-size"),
            clipboardTagFontSize: fontSize("--clipboard-tag-font-size")
        }, () => !hiddenRef.current && hoverAnchorRef.current !== null
            && contentIdentity.current.generation === generation);
    };

    useEffect(() => { void hideCompactPreviewGlobal(previewOwner.current); }, [item]);

    // Sync local state when prop changes (e.g. when editor opens)
    useEffect(() => {
        setLocalTagInput(tagInput);
    }, [tagInput]);

    useEffect(() => {
        setSnapshotFailed(false);
        setRichImageFallbackFailed(false);
    }, [item.id, item.html_content, richTextSnapshotPreview, compactMode]);

    useEffect(() => {
        if (richSnapshotFallbackTimerRef.current) {
            clearTimeout(richSnapshotFallbackTimerRef.current);
            richSnapshotFallbackTimerRef.current = null;
        }
        if (!useSnapshotPreviewImage) return;

        // Safety net: some WebView failures do not reliably fire <img onError>.
        richSnapshotFallbackTimerRef.current = setTimeout(() => {
            const img = richSnapshotImgRef.current;
            if (!img || !img.complete || img.naturalWidth <= 0 || img.naturalHeight <= 0) {
                richPreviewFailureLog("snapshot image timeout -> fallback to html", {
                    itemId: item.id,
                    hasImageElement: !!img,
                    complete: img?.complete ?? false,
                    naturalWidth: img?.naturalWidth ?? 0,
                    naturalHeight: img?.naturalHeight ?? 0
                });
                setSnapshotFailed(true);
            }
        }, 700);

        return () => {
            if (richSnapshotFallbackTimerRef.current) {
                clearTimeout(richSnapshotFallbackTimerRef.current);
                richSnapshotFallbackTimerRef.current = null;
            }
        };
    }, [useSnapshotPreviewImage, effectiveRichTextSnapshotSrc, item.id]);

    useEffect(() => {
        if (isEditingTags && tagInputRef.current) {
            tagInputRef.current.focus();
        }
    }, [isEditingTags]);

    useEffect(() => {
        if (!compactMode || isSensitiveHidden) {
            hideCompactPreview();
        }
    }, [compactMode, isSensitiveHidden]);

    useEffect(() => {
        return () => {
            if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
            hoverAnchorRef.current = null;
            void hideCompactPreviewGlobal(previewOwner.current);
        };
    }, []);

    useEffect(() => {
        fetchTransformKinds()
            .then((kinds) => {
                if (kinds && Array.isArray(kinds)) {
                    // The answer is the same for every row and is usually already
                    // in hand by the time this resolves. A freshly allocated array
                    // with identical contents is still a new value to a state
                    // setter, and it arrives after mount, so it re-rendered the
                    // whole row a second time for every row that scrolled into
                    // view. Handing back the previous list settles for "no
                    // change" and skips that render.
                    setTransformKinds((prev) => (sameListById(prev, kinds) ? prev : kinds));
                }
            })
            .catch(() => {});
    }, []);

    // The row is reused for a different entry whenever the list shifts under
    // it, so the subscription reads the id it is currently showing instead of
    // re-subscribing on every shift.
    const ocrItemIdRef = useRef(item.id);
    ocrItemIdRef.current = item.id;

    useEffect(() => {
        return subscribeOcrComplete(
            (itemId) => itemId === ocrItemIdRef.current,
            (payload) => {
                setOcrStatus(payload.status);
                const access = beginPrivacyAccess();
                if (!access || hiddenRef.current) { setOcrText(null); return; }
                const generation = contentIdentity.current.generation;
                void invokePrivacyBound<ClipboardEntry>("get_privacy_entry", { id: ocrItemIdRef.current, expectedHostInstanceId: contentIdentity.current.item.host_instance_id, expectedEpoch: access.epoch })
                    .then(entry => { if (access.isCurrent() && !hiddenRef.current && generation === contentIdentity.current.generation) { setOcrSource(contentIdentity.current.item); setOcrText(entry.ocr_text ?? null); } })
                    .catch(() => { if (generation === contentIdentity.current.generation) setOcrText(null); });
            }
        );
    }, []);

    useEffect(() => {
        setOcrSource(item);
        setOcrText(item.ocr_text ?? null);
        setOcrStatus(item.ocr_status ?? null);
        setOcrTextExpanded(false);
    }, [item]);

    const [ocrRunning, setOcrRunning] = useState(false);
    const shareInFlightRef = useRef(false);

    const handleOcr = useCallback(() => {
        if (ocrRunning || item.content_type !== "image") return;
        const access = beginPrivacyAccess();
        if (!access || isSensitiveHidden) { onTransformItemError?.("privacy_read", t("privacy_reveal_first")); return; }
        const generation = contentIdentity.current.generation;
        setOcrSource(item);
        setOcrRunning(true);
        setOcrStatus("processing");
        invokePrivacyBound("recognize_clipboard_image", { itemId: item.id, expectedHostInstanceId: item.host_instance_id, expectedPrivacyEpoch: access.epoch })
            .then((result: unknown) => {
                if (!access.isCurrent() || generation !== contentIdentity.current.generation || hiddenRef.current) return;
                const res = result as { text: string; status: string };
                setOcrText(res.text || null);
                setOcrStatus(res.status);
            })
            .catch((err) => {
                if (!access.isCurrent() || generation !== contentIdentity.current.generation || hiddenRef.current) return;
                const msg = err instanceof Error ? err.message : String(err);
                setOcrStatus("failed");
                setContextMenuState(null);
                console.error("OCR failed:", msg);
            })
            .finally(() => setOcrRunning(false));
    }, [item.id, item.host_instance_id, item.content_type, ocrRunning, isSensitiveHidden, onTransformItemError, t]);

    const handleTransform = useCallback((kind: string) => {
        const access = beginPrivacyAccess();
        if (!access || isSensitiveHidden) { onTransformItemError?.(kind, t("privacy_reveal_first")); return; }
        const generation = contentIdentity.current.generation;
        const proceed = (text: string) => {
            if (!access.isCurrent() || generation !== contentIdentity.current.generation || hiddenRef.current) return;
            invoke("copy_to_clipboard", {
                content: text,
                contentType: "text",
                paste: false,
                id: 0,
                deleteAfterUse: false,
                pasteWithFormat: false,
                moveToTop: false,
                pasteImageAsBase64: false,
                expectedPrivacyEpoch: access.epoch,
                expectedHostInstanceId: item.host_instance_id,
                derivedFromId: isSensitiveEntry(item) ? item.id : undefined
            })
                .then(() => onTransformItemSuccess?.(kind))
                .catch((err) => {
                    const msg = err instanceof Error ? err.message : String(err);
                    onTransformItemError?.(kind, msg);
                });
        };
        onTransformItem?.(kind);
        const transform = (entry: ClipboardEntry) => {
            if (!access.isCurrent() || generation !== contentIdentity.current.generation || hiddenRef.current) return;
            return invoke<string>("transform_text", { text: entry.content, kind })
            .then((result) => {
                if (!access.isCurrent() || generation !== contentIdentity.current.generation || hiddenRef.current) return;
                if (result) {
                    proceed(result);
                } else {
                    onTransformItemError?.(kind, "transform returned empty result");
                }
            })
            .catch((err) => {
                const msg = err instanceof Error ? err.message : String(err);
                onTransformItemError?.(kind, msg);
            });
        };
        if (loadFullItem) {
            void loadFullItem().then(transform).catch((error: unknown) => {
                onTransformItemError?.(kind, error instanceof Error ? error.message : String(error));
            });
        } else {
            void transform(item);
        }
    }, [item, loadFullItem, onTransformItem, onTransformItemError, onTransformItemSuccess, isSensitiveHidden, t]);

    const handleImageBase64Copy = useCallback(async () => {
        if (item.content_type !== "image") return;
        const access = beginPrivacyAccess();
        if (!access || isSensitiveHidden) { onTransformItemError?.("privacy_read", t("privacy_reveal_first")); return; }
        try {
            await invoke("copy_to_clipboard", {
                content: item.content,
                contentType: "image",
                paste: false,
                id: item.id,
                deleteAfterUse: false,
                pasteWithFormat: false,
                moveToTop: false,
                pasteImageAsBase64: true,
                expectedHostInstanceId: item.host_instance_id,
                expectedPrivacyEpoch: isSensitiveEntry(item) ? access.epoch : undefined
            });
            onTransformItemSuccess?.("image_base64");
        } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            onTransformItemError?.("image_base64", msg);
        }
    }, [item, onTransformItemError, onTransformItemSuccess, isSensitiveHidden, t]);

    const handlePasteOcr = useCallback(async () => {
        if (!ocrText) return;
        const access = beginPrivacyAccess();
        if (!access || isSensitiveHidden) { onTransformItemError?.("privacy_read", t("privacy_reveal_first")); return; }
        try {
            await invoke("copy_to_clipboard", {
                content: ocrText,
                contentType: "text",
                paste: false,
                id: 0,
                deleteAfterUse: false,
                pasteWithFormat: false,
                moveToTop: false,
                pasteImageAsBase64: false,
                expectedPrivacyEpoch: access.epoch,
                expectedHostInstanceId: item.host_instance_id,
                derivedFromId: isSensitiveEntry(item) ? item.id : undefined,
            });
            onTransformItemSuccess?.("paste_ocr");
        } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            onTransformItemError?.("paste_ocr", msg);
        }
    }, [ocrText, onTransformItemError, onTransformItemSuccess, isSensitiveHidden, t, item]);

    const handleShare = useCallback(async () => {
        if (shareInFlightRef.current) {
            onTransformItemError?.("share", t("share_in_progress"));
            return;
        }
        shareInFlightRef.current = true;
        try {
            const access = beginPrivacyAccess();
            if (!access || isSensitiveHidden) { onTransformItemError?.("privacy_read", t("privacy_reveal_first")); return; }
            const generation = contentIdentity.current.generation;
            if (navigator.share) {
                const content = loadFullItem ? (await loadFullItem()).content : item.content;
                if (!access.isCurrent() || generation !== contentIdentity.current.generation || hiddenRef.current) return;
                await navigator.share({ text: content });
                return;
            }
            if (!access.isCurrent()) return;
            await invoke("copy_to_clipboard", {
                id: item.id, content: "", contentType: item.content_type, paste: false,
                deleteAfterUse: false, pasteWithFormat: item.content_type === "rich_text", moveToTop: false,
                pasteImageAsBase64: false,
                expectedHostInstanceId: item.host_instance_id,
                expectedPrivacyEpoch: isSensitiveEntry(item) ? access.epoch : undefined,
            });
        } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            onTransformItemError?.("share", msg);
        } finally {
            shareInFlightRef.current = false;
        }
    }, [item, loadFullItem, onTransformItemError, t, isSensitiveHidden]);

    const handleContextMenuAction = useCallback((action: string) => {
        switch (action) {
            case "copy":
                onCopy(false, false);
                break;
            case "editTags":
                onToggleTagEditor(new MouseEvent("contextmenu") as never);
                break;
            case "qrCode":
                onQRCode?.();
                break;
            case "delete":
                onDelete(new MouseEvent("contextmenu") as never);
                break;
            case "pin":
                void runMetadataAction(intent => onTogglePin(new MouseEvent("contextmenu") as never, intent));
                break;
            case "share":
                void handleShare();
                break;
            case "imageBase64":
                void handleImageBase64Copy();
                break;
        }
    }, [onCopy, onToggleTagEditor, onQRCode, onDelete, onTogglePin, handleShare, handleImageBase64Copy, runMetadataAction]);
    const handlePrivacyFeedback = useCallback(async (action: PrivacyFeedbackAction) => {
        const access = beginPrivacyAccess();
        if (!access || item.id === 0 || privacyFeedbackPending) return;
        setPrivacyFeedbackPending(true);
        let release: (() => void) | undefined;
        try {
            release = await holdNativeDialogBlur();
            const decision = await invokePrivacyBound<{ reason: string }>("get_privacy_decision", { id: item.id, expectedHostInstanceId: item.host_instance_id, expectedEpoch: access.epoch });
            const hint = decision.reason === "scan_incomplete" ? t("privacy_scan_incomplete")
                : decision.reason === "suspected_sensitive" ? t("privacy_scan_suspected") : "";
            const prompt = t(action === "not_sensitive" ? "privacy_feedback_unprotect_confirm"
                : action === "exact_exception" ? "privacy_feedback_exact_confirm" : "privacy_feedback_confirm");
            const confirmed = await ask(hint ? `${hint}\n\n${prompt}` : prompt, { title: t("privacy_feedback_title"), kind: "warning" });
            await refreshPrivacyAfterConfirmation(confirmed ? access.epoch : undefined);
            if (!confirmed) return;
            if (getPrivacyViewState().phase !== "ready" || getPrivacyViewState().snapshot?.privacy_epoch !== access.epoch) throw new Error("PrivacyEpochChanged");
            await submitPrivacyFeedback(item.id, action, item.host_instance_id);
            onTransformItemSuccess?.("privacy_feedback");
        } catch {
            onTransformItemError?.("privacy_feedback", t("privacy_action_failed"));
        } finally {
            release?.();
            setPrivacyFeedbackPending(false);
        }
    }, [item.id, item.host_instance_id, privacyFeedbackPending, onTransformItemError, onTransformItemSuccess, t]);

    useEffect(() => {
        if (!contextMenuState && !privacyFeedbackPending && !metadataActionPending) return;
        setIgnoreBlurSafe(true);
        const handleClick = () => setContextMenuState(null);
        document.addEventListener("click", handleClick);
        return () => {
            document.removeEventListener("click", handleClick);
            setIgnoreBlurSafe(false);
        };
    }, [contextMenuState, privacyFeedbackPending, metadataActionPending]);

    useEffect(() => {
        const handleOtherMenuOpen = (e: Event) => {
            const customEvent = e as CustomEvent<{ id: string }>;
            if (customEvent.detail?.id !== String(item.id) && contextMenuState) {
                setContextMenuState(null);
            }
        };
        window.addEventListener(CONTEXT_MENU_OPEN_EVENT, handleOtherMenuOpen);
        return () => window.removeEventListener(CONTEXT_MENU_OPEN_EVENT, handleOtherMenuOpen);
    }, [contextMenuState, item.id]);

    const renderFilePreview = () => {
        if (item.file_preview_exists === false) {
            return (
                <div className="file-thumbnail-card error-bg" title={t('file_deleted') || "File Deleted"}>
                    <div className="file-icon-wrapper error-icon">
                        <FileQuestion size={24} />
                    </div>
                    <div className="file-info-wrapper">
                        <div className="file-name error-text">{t('file_deleted') || "Deleted"}</div>
                        <div className="file-hint error-text">{item.content}</div>
                    </div>
                </div>
            );
        }

        const paths = item.content.split('\n').filter(p => p.trim());
        if (paths.length === 0) {
            return (
                <div className="file-thumbnail-card">
                    <div className="file-icon-wrapper"><File size={24} /></div>
                    <div className="file-info-wrapper"><div className="file-name">{item.preview}</div></div>
                </div>
            );
        }
        if (paths.length > 1) {
            return (
                <div className="file-thumbnail-card" title={item.content}>
                    <div className="file-icon-wrapper">
                        <Files size={24} />
                    </div>
                    <div className="file-info-wrapper">
                        <div className="file-name">{paths.length} {t('items')}</div>
                        <div className="file-hint">{paths[0].split(/[\\/]/).pop()} ...</div>
                    </div>
                </div>
            );
        }

        const filePath = paths[0];
        const fileName = filePath.split(/[\\/]/).pop();
        const dirPath = filePath.split(/[\\/]/).slice(0, -1).join('\\');

        return (
            <div className="file-thumbnail-card" title={item.content}>
                <div className="file-icon-wrapper">
                    {getFileIcon(filePath)}
                </div>
                <div className="file-info-wrapper">
                    <div className="file-name">{fileName}</div>
                    <div className="file-hint">{dirPath}</div>
                </div>
            </div>
        );
    };

    return (
        <motion.div
            id={id}
            data-test-clipboard-item
            layout={!disableLayout}
            initial={false}
            animate={disableLayout ? undefined : { marginBottom: 0 }}
            exit={disableLayout ? undefined : { opacity: 0, scale: 0.95 }}
            transition={disableLayout ? undefined : { duration: 0.1 }}
            className={`history-item ${isSelected ? "selected" : ""} ${compactMode ? "compact" : ""} ${item.is_pinned ? "pinned" : ""} ${className || ''}`}
            onMouseDown={(e) => {
                if (!windowPinned) return;
                const target = e.target as HTMLElement;
                if (target.closest('button, input, textarea, [role="button"], .drag-handle')) {
                    return;
                }
                if (target.closest('a')) {
                    return;
                }
                // Removed e.preventDefault() to allow blur of search box
            }}
            onClick={(e) => {
                const target = e.target as HTMLElement;
                if (target.closest('button') || target.closest('input') || target.closest('textarea')) {
                    return;
                }
                void hideCompactPreviewGlobal();
                // Prevent link navigation - we want to copy, not open links
                if (target.closest('a')) {
                    e.preventDefault();
                }
                onCopy(false, false);
                onSelect();
            }}
            onContextMenu={(e) => {
                const target = e.target as HTMLElement;
                if (target.closest('button') || target.closest('input') || target.closest('textarea')) {
                    return;
                }
                if (hoverTimerRef.current) {
                    clearTimeout(hoverTimerRef.current);
                    hoverTimerRef.current = null;
                }
                void hideCompactPreviewGlobal();
                e.preventDefault();
                e.stopPropagation();
                window.dispatchEvent(new CustomEvent(CONTEXT_MENU_OPEN_EVENT, { detail: { id: item.id } }));
                const anchorRect = toCompactPreviewRect(e.currentTarget.getBoundingClientRect());
                const viewport = {
                    width: window.innerWidth,
                    height: window.innerHeight,
                    scale: window.devicePixelRatio || 1
                };
                const currentWindowLabel = windowLabelRef.current;
                invoke<boolean>("is_window_topmost", { label: COMPACT_PREVIEW_LABEL })
                    .then((isCompactPreviewOnTop) => {
                        const point = resolveContextMenuPoint({
                            clientX: e.clientX,
                            clientY: e.clientY,
                            anchorRect,
                            viewport,
                            currentWindowLabel,
                            isCompactPreviewOnTop
                        });
                        setContextMenuState(point);
                    })
                    .catch(() => {
                        const point = resolveContextMenuPoint({
                            clientX: e.clientX,
                            clientY: e.clientY,
                            anchorRect,
                            viewport,
                            currentWindowLabel,
                            isCompactPreviewOnTop: false
                        });
                        setContextMenuState(point);
                    });
            }}
            onMouseEnter={(e) => {
                if (!compactMode || isSensitiveHidden) return;
                if (contextMenuState) return;
                compactPreviewLog("mouseenter schedule preview", { itemId: item.id });
                const target = e.currentTarget;

                // Measured in the frame rather than here, for the same reason
                // `onMouseMove` does it: a row entering under a stationary
                // pointer can land while the list is still laying out, and a
                // geometry read in the handler may force a synchronous layout.
                // What that buys is measured and worth stating precisely: the
                // synchronous reads drop to zero, and the layout count does not
                // move, so the gain is getting the reads off the handler's
                // synchronous path rather than fewer layouts. The hover timer
                // does not fire immediately, so a frame's delay changes nothing
                // about what the preview is placed against.
                hoverAnchorThrottle.schedule({
                    clientX: e.clientX,
                    clientY: e.clientY,
                    screenX: e.screenX,
                    screenY: e.screenY,
                    element: target
                });

                // Clear any pending hide timer
                if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);

                // Set a delay to show
                hoverTimerRef.current = setTimeout(() => {
                    if (!target.isConnected) return;
                    const anchor = hoverAnchorRef.current;
                    if (!anchor) return;
                    compactPreviewLog("mouseenter timer fired, show preview", { itemId: item.id });
                    showCompactPreview(anchor);
                }, 300);
            }}
            onMouseMove={(e) => {
                if (!compactMode) return;
                if (contextMenuState) return;
                // The anchor is only read when the hover delay ends, when the preview
                // timer fires, so the newest position in the frame is exactly
                // as good as the newest position in the event queue. Measuring
                // on every event was a forced synchronous layout per event, on
                // the hottest path in the list; the element is captured here
                // because `currentTarget` is gone by the time the frame runs.
                hoverAnchorThrottle.schedule({
                    clientX: e.clientX,
                    clientY: e.clientY,
                    screenX: e.screenX,
                    screenY: e.screenY,
                    element: e.currentTarget
                });
            }}
            onMouseLeave={() => {
                if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
                // Drop a frame that is still queued: applying it after the
                // pointer has already left would resurrect an anchor for an
                // item the user is no longer on.
                hoverAnchorThrottle.cancel();
                hoverAnchorRef.current = null;
                compactPreviewLog("mouseleave hide preview", { itemId: item.id });
                hideCompactPreview();
            }}
        >
            <div className="item-meta">
                <div className="item-meta-left">
                    {dragControls && (
                        <div
                            className="drag-handle"
                            onPointerDown={(e) => dragControls.start(e)}
                            onClick={(e) => e.stopPropagation()}
                            style={{
                                cursor: 'grab',
                                opacity: 0.5,
                                display: 'flex',
                                alignItems: 'center',
                                touchAction: 'none'
                            }}
                        >
                            <GripVertical size={14} />
                        </div>
                    )}
                    <div className="app-info">
                        {item.is_pinned && !dragControls && <Pin size={10} style={{ color: 'var(--accent-color)', marginRight: '-2px' }} />}
                        {renderSourceAppIcon(sourceAppIcon, item.content_type, item.source_app)}
                        <span>{item.source_app}</span>
                    </div>
                </div>

                <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <div className="item-actions">
                        {onEdit && !isSensitiveHidden && (item.content_type === "text" || item.content_type === "code") && (
                            <button className="btn-icon" title={t('common_edit')} onClick={(event) => {
                                event.stopPropagation();
                                onEdit();
                            }}><Edit2 size={12} /></button>
                        )}
                        {showSensitiveVisibilityControl && isSensitiveEntry(item) && (
                            <button
                                className={`btn-icon ${isRevealed ? "active" : ""}`}
                                onClick={onToggleReveal}
                                title={isRevealed ? t('hide') : t('reveal')}
                            >
                                {isRevealed ? <EyeOff size={12} /> : <Eye size={12} />}
                            </button>
                        )}
                        <button
                            className="btn-icon"
                            onClick={onOpen}
                            title={t('open')}
                        >
                            <ExternalLink size={12} />
                        </button>
                        <button
                            className={`btn-icon ${item.is_pinned ? "active" : ""}`}
                            onClick={event => { event.stopPropagation(); void runMetadataAction(intent => onTogglePin(event, intent)); }}
                            disabled={metadataActionPending}
                            title={item.is_pinned ? t('unpin') : t('pin')}
                        >
                            {item.is_pinned ? <PinOff size={12} /> : <Pin size={12} />}
                        </button>
                        <button
                            className={`btn-icon ${item.tags && item.tags.length > 0 ? "active" : ""}`}
                            onClick={onToggleTagEditor}
                            title="Tags"
                        >
                            <Tag size={12} />
                        </button>
                        <button className="btn-icon" onClick={onDelete} title={t('delete')}>
                            <X size={12} />
                        </button>
                    </div>
                    <div className="app-info" style={{ opacity: 0.6, fontSize: '10px', display: 'flex', gap: '6px', alignItems: 'center' }}>
                        {ocrStatus && ocrStatus !== "pending" && (
                            <span
                                data-testid="ocr-badge"
                                className="ocr-badge"
                                style={{
                                    fontSize: '9px',
                                    padding: '1px 4px',
                                    borderRadius: '3px',
                                    background: ocrStatus === "done"
                                        ? 'var(--accent-color, #4a9eff)'
                                        : ocrStatus === "processing"
                                            ? 'var(--text-secondary, #888)'
                                            : ocrStatus === "failed"
                                                ? 'var(--error-color, #e74c3c)'
                                                : 'var(--text-tertiary, #666)',
                                    color: '#fff',
                                    whiteSpace: 'nowrap',
                                }}
                            >
                                {ocrStatus === "processing"
                                    ? t('ocr_processing')
                                    : ocrStatus === "done"
                                        ? t('ocr_done_label')
                                        : ocrStatus === "failed"
                                            ? t('ocr_failed_label')
                                            : t('ocr_unsupported_label')}
                            </span>
                        )}
                        <span>{getConciseTime(item.timestamp, language)}</span>
                    </div>
                </div>
            </div>

            {compactMode && item.is_pinned && (
                <div className="compact-pinned-indicator" title={t('pinned')}>
                    <Pin size={10} fill="currentColor" />
                </div>
            )}
            <div className={`content-preview ${item.content_type === 'rich_text' ? 'rich-text' : ''} ${isSensitiveHidden ? 'privacy-concealed' : ''}`}
                data-privacy-protected={isSensitiveEntry(item) ? "true" : undefined}
                onDragStart={event => { if (isSensitiveEntry(item)) event.preventDefault(); }}>
                {isSensitiveHidden ? <div className="privacy-hidden-content">{t("privacy_sensitive_hidden")}</div>
                    : item.content_type === "file" && !item.content ? <div className="privacy-file-metadata">{item.preview}</div>
                    : item.content_type === "image" ? (
                    <div style={{ position: 'relative' }}>
                        {item.is_external && item.file_preview_exists === false ? (
                            <div className="image-preview error-placeholder" style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', background: 'var(--bg-secondary)', color: 'var(--text-secondary)', height: '100px', fontSize: '12px' }}>
                                <ImageOff size={24} style={{ marginBottom: '8px', opacity: 0.5 }} />
                                <span>{t('image_deleted') || 'Image Deleted'}</span>
                            </div>
                        ) : imagePreview.pending ? (
                            <div className="image-preview image-preview-pending" aria-hidden="true" />
                        ) : imagePreview.src ? (
                            <img
                                src={imagePreview.src}
                                alt={t('image_preview')}
                                className="image-preview"
                                loading="lazy"
                                style={isSensitiveHidden ? { filter: 'blur(8px)' } : {}}
                                onError={(e) => {
                                    // Fallback for load errors even if backend said it exists (e.g. deleted after fetch)
                                    e.currentTarget.style.display = 'none';
                                    e.currentTarget.parentElement?.classList.add('image-load-error');
                                }}
                            />
                        ) : <div className="image-preview" aria-label={t('common_loading')}><File size={24} /></div>}
                        {isSensitiveHidden && (
                            <div style={{ position: 'absolute', top: '50%', left: '50%', transform: 'translate(-50%, -50%)', fontWeight: 'bold', opacity: 0.5, fontSize: '10px' }}>
                                SENSITIVE
                            </div>
                        )}
                    </div>
                ) : item.content_type === "video" ? (
                    <div className="video-thumbnail-card">
                        <div className="video-thumbnail-wrapper">
                            {/^data:video\/(mp4|webm);base64,[A-Za-z0-9+/=]+$/.test(item.content) && <video
                                src={item.content}
                                preload="metadata"
                                muted
                                playsInline
                                className="video-thumbnail-element"
                                onLoadedMetadata={(e) => seekVideoPreviewFrame(e.currentTarget)}
                            />}
                            <div className="video-play-overlay">
                                <Video size={16} />
                            </div>
                        </div>
                        <div className="video-info-wrapper">
                            <div className="video-name">{item.preview || item.content.split(/[\\/]/).pop()}</div>
                        </div>
                    </div>
                ) : item.content_type === "file" ? (
                    renderFilePreview()
                ) : item.content_type === "rich_text" && item.html_content && !isSensitiveHidden ? (
                    richTextPreviewSrc ? (
                        <img
                            ref={richSnapshotImgRef}
                            src={richTextPreviewSrc}
                            alt="rich text preview"
                            onLoad={() => {
                                if (useSnapshotPreviewImage && richSnapshotFallbackTimerRef.current) {
                                    clearTimeout(richSnapshotFallbackTimerRef.current);
                                    richSnapshotFallbackTimerRef.current = null;
                                }
                            }}
                            onError={() => {
                                if (useRichImageFallback) {
                                    richPreviewFailureLog("fallback image load error -> switch to snapshot", {
                                        itemId: item.id,
                                        srcLength: (richTextPreviewSrc || "").length
                                    });
                                    setRichImageFallbackFailed(true);
                                    return;
                                }
                                if (richSnapshotFallbackTimerRef.current) {
                                    clearTimeout(richSnapshotFallbackTimerRef.current);
                                    richSnapshotFallbackTimerRef.current = null;
                                }
                                if (effectiveRichTextSnapshotSrc) {
                                    richPreviewFailureLog("snapshot image load error -> fallback to html", {
                                        itemId: item.id,
                                        srcLength: (richTextPreviewSrc || "").length
                                    });
                                    setSnapshotFailed(true);
                                }
                            }}
                            style={{
                                width: 'auto',
                                maxWidth: '100%',
                                maxHeight: `${richTextSnapshotDisplayMaxHeight}px`,
                                display: 'block',
                                marginRight: 'auto',
                                pointerEvents: 'none',
                                borderRadius: '4px',
                                maskImage: 'linear-gradient(to bottom, black 78%, transparent 100%)',
                                WebkitMaskImage: 'linear-gradient(to bottom, black 78%, transparent 100%)'
                            }}
                        />
                    ) : (
                        <HtmlContent
                            className="rich-text-preview"
                            htmlContent={richTextCleanHtml || item.html_content}
                            fallbackText={item.preview}
                            preview={true}
                            style={{
                                maxHeight: `${richTextSnapshotDisplayMaxHeight}px`,
                                overflow: 'hidden',
                                fontSize: 'var(--clipboard-item-font-size)',
                                lineHeight: '1.4',
                                position: 'relative',
                                pointerEvents: 'none', // Prevent interacting with links in the list
                                maskImage: 'linear-gradient(to bottom, black 70%, transparent 100%)',
                                WebkitMaskImage: 'linear-gradient(to bottom, black 70%, transparent 100%)'
                            }}
                        />
                    )
                ) : (
                    isSensitiveHidden
                        ? (
                            <div style={{ minHeight: '24px', opacity: 0.6, fontStyle: 'italic', display: 'flex', alignItems: 'center', gap: '8px', fontFamily: 'var(--font-mono)' }}>
                                <span style={{ letterSpacing: '1px' }}>
                                    {item.preview.substring(0, 3)}...
                                </span>
                                {!loadFullItem && <span style={{ fontSize: '10px', opacity: 0.7 }}>
                                    ({item.content.length} {t('chars') || 'chars'})
                                </span>}
                            </div>
                        )
                        : item.preview
                )}
            </div>

            {ocrStatus === "done" && ocrText && (
                <div
                    data-testid="ocr-section"
                    data-privacy-protected={isSensitiveEntry(item) ? "true" : undefined}
                    onDragStart={event => { if (isSensitiveEntry(item)) event.preventDefault(); }}
                    className="ocr-text-section"
                    style={{
                        marginTop: '4px',
                        borderTop: '1px solid var(--border-color, rgba(128,128,128,0.2))',
                        paddingTop: '4px',
                    }}
                >
                    <button
                        data-testid="ocr-toggle"
                        className="btn-icon"
                        onClick={(e) => {
                            e.stopPropagation();
                            setOcrTextExpanded((prev) => !prev);
                        }}
                        title={t('ocr_toggle_label')}
                        style={{
                            fontSize: '9px',
                            padding: '1px 4px',
                            background: 'none',
                            border: 'none',
                            color: 'var(--text-secondary, #888)',
                            cursor: 'pointer',
                            display: 'flex',
                            alignItems: 'center',
                            gap: '4px',
                        }}
                    >
                        {ocrTextExpanded ? '▼' : '▶'} {t('ocr_done_label')}
                    </button>
                    {ocrTextExpanded && (
                        <div
                            data-testid="ocr-text"
                            className="ocr-text-content"
                            style={{
                                fontSize: '11px',
                                color: 'var(--text-secondary, #888)',
                                fontFamily: 'var(--font-mono, monospace)',
                                lineHeight: '1.4',
                                maxHeight: '80px',
                                overflow: 'auto',
                                whiteSpace: 'pre-wrap',
                                wordBreak: 'break-all',
                                padding: '2px 4px',
                                marginTop: '2px',
                                borderRadius: '3px',
                                background: 'var(--bg-tertiary, rgba(128,128,128,0.05))',
                                userSelect: 'text',
                                WebkitUserSelect: 'text',
                            }}
                        >
                            {ocrText}
                        </div>
                    )}
                </div>
            )}

            {(item.tags?.length > 0 || isEditingTags) && (
                <div
                    className="item-tags-container"
                    style={{
                        marginTop: '2px',
                        display: 'flex',
                        flexWrap: 'wrap',
                        justifyContent: 'flex-end',
                        gap: '4px',
                        paddingTop: '0'
                    }}>
                    {item.tags?.map(tag => (
                        <span
                            key={tag}
                            className="tag-chip"
                            style={{
                                background: tagColors?.[tag] || getTagColor(tag, theme),
                                display: 'flex',
                                alignItems: 'center',
                                gap: '4px'
                            }}
                        >
                            {tag}
                            {isEditingTags && (
                                <button
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        void runMetadataAction(intent => onTagDelete(tag, intent));
                                    }}
                                    style={{ background: 'none', border: 'none', padding: 0, color: 'rgba(255,255,255,0.7)', cursor: 'pointer', display: 'flex' }}
                                >
                                    <X size={8} />
                                </button>
                            )}
                        </span>
                    ))}

                    {isEditingTags && (
                        <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                            <input
                                ref={tagInputRef}
                                type="text"
                                value={localTagInput}
                                onCompositionStart={() => {
                                    isComposing.current = true;
                                }}
                                onCompositionEnd={(e) => {
                                    isComposing.current = false;
                                    const val = (e.target as HTMLInputElement).value;
                                    setLocalTagInput(val);
                                    onTagInput(val);
                                }}
                                onMouseDown={() => {
                                    invoke('activate_window_focus').catch(console.error);
                                }}
                                onFocus={() => {
                                    invoke('activate_window_focus').catch(console.error);
                                }}
                                onChange={(e) => {
                                    const val = e.target.value;
                                    setLocalTagInput(val);
                                    if (!isComposing.current) {
                                        onTagInput(val);
                                    }
                                }}
                                onKeyDown={(e) => {
                                    if (e.key === 'Enter' && !isComposing.current) {
                                        void runMetadataAction(intent => onTagAdd(intent));
                                    }
                                }}
                                className="tag-input"
                                placeholder="New tag..."
                                style={{
                                    background: 'var(--bg-input)',
                                    border: 'none',
                                    borderRadius: '0',
                                    padding: '2px 4px',
                                    fontSize: '10px',
                                    color: 'var(--text-primary)',
                                    width: '60px',
                                    outline: 'none'
                                }}
                                onClick={e => e.stopPropagation()}
                            />
                            <button
                                onClick={(e) => {
                                    e.stopPropagation();
                                    void runMetadataAction(intent => onTagAdd(intent));
                                }}
                                className="btn-icon"
                                style={{ padding: '2px', height: '16px', width: '16px' }}
                            >
                                <Plus size={10} />
                            </button>
                        </div>
                    )}
                </div>
            )}
            {contextMenuState && (
                <ItemContextMenu
                    x={contextMenuState.x}
                    y={contextMenuState.y}
                    entry={item}
                    onSelect={() => setContextMenuState(null)}
                    onClose={() => setContextMenuState(null)}
                    onCopy={(withFormat) => onCopy(!!withFormat, false)}
                    onEdit={isSensitiveHidden ? undefined : onEdit}
                    editLabel={t('common_edit')}
                    onEditTags={() => handleContextMenuAction("editTags")}
                    onQRCode={() => handleContextMenuAction("qrCode")}
                    onDelete={() => handleContextMenuAction("delete")}
                    onPin={() => handleContextMenuAction("pin")}
                    onShare={() => handleContextMenuAction("share")}
                    onImageBase64={() => handleContextMenuAction("imageBase64")}
                    onTransform={handleTransform}
                    onOcr={handleOcr}
                    onPasteOcr={() => handlePasteOcr()}
                    onPrivacyFeedback={action => { void handlePrivacyFeedback(action); }}
                    transformKinds={transformKinds}
                    language={language}
                    ocrRunning={ocrRunning}
                    ocrText={ocrText}
                />
            )}
        </motion.div>
    );
};

export default memo(ClipboardItem);
