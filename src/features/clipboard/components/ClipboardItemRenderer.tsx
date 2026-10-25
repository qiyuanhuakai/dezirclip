import { memo, useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, MouseEvent, SetStateAction } from "react";
import type { DragControls } from "framer-motion";
import ClipboardItem, { hideCompactPreviewGlobal } from "./ClipboardItem";
import type { ClipboardEntry, Locale } from "../../../shared/types";
import { beginPrivacyAccess, forgetPrivacyReveal, getPrivacyEntry, invokePrivacyBound, registerPrivacyReveal, subscribePrivacyInvalidation } from "../../../shared/lib/privacySession";
import { concealSensitiveEntry, isSensitiveEntry } from "../../../shared/lib/privacyRuntime";
import { clearRichTextSnapshots } from "../../../shared/lib/richTextSnapshot";
import type { PrivacyMetadataIntent } from "../../../shared/lib/privacyMetadataIntent";
import { matchesEntryIdentity } from "../../../shared/lib/historyIdentity";

/**
 * Binds a clipboard row to its handlers.
 *
 * The row is rendered from a plain function that the virtualized list calls for
 * every visible index on every render, so it could only ever hand ClipboardItem
 * fresh inline callbacks. ClipboardItem is memoised, and a memo boundary that
 * sees a new function identity every time never bails out — moving the keyboard
 * selection re-rendered every visible row, including the 1600-line component
 * behind each one.
 *
 * Here the handlers are built with useCallback instead, keyed on the values they
 * actually read, and the whole component is memoised. The values the virtualized
 * list passes down are already stable: a selection move changes `isSelected` on
 * two rows and nothing else, so those two re-render and the rest bail out.
 */

export interface ClipboardItemRendererProps {
    item: ClipboardEntry;
    index: number;
    isSelected: boolean;
    windowPinned: boolean;
    isSensitiveHidden: boolean;
    isRevealed: boolean;
    isEditingTags: boolean;
    tagInput: string;
    tagColors: Record<string, string>;
    theme: string;
    language: Locale;
    t: (key: string) => string;
    compactMode: boolean;
    richTextSnapshotPreview: boolean;
    dragControls?: DragControls;
    disableLayout?: boolean;

    setSelectedIndex: Dispatch<SetStateAction<number>>;
    copyToClipboard: (
        id: number,
        content: string,
        contentType: string,
        pasteWithFormat?: boolean,
        pasteImageAsBase64?: boolean,
        expectedPrivacyEpoch?: number,
        derivedFromId?: number,
        hostInstanceId?: string
    ) => Promise<void>;
    setRevealedIds: Dispatch<SetStateAction<Set<number>>>;
    openContent: (item: ClipboardEntry, expectedPrivacyEpoch?: number) => void;
    togglePin: (event: MouseEvent, id: number, isPinned: boolean, hostInstanceId?: string, intent?: PrivacyMetadataIntent) => void | Promise<void>;
    deleteEntry: (event: MouseEvent, id: number, hostInstanceId?: string) => void;
    setEditingTagsId: Dispatch<SetStateAction<number | null>>;
    setTagInput: Dispatch<SetStateAction<string>>;
    handleUpdateTags: (id: number, tags: string[], hostInstanceId?: string, intent?: PrivacyMetadataIntent) => void | Promise<void>;
    onQRCode: (item: ClipboardEntry) => void;
    onTransformError: (item: ClipboardEntry, kind: string, message: string) => void;
    onTransformSuccess: (item: ClipboardEntry, kind: string) => void;
}

const ClipboardItemRenderer = ({
    item,
    index,
    isSelected,
    windowPinned,
    isSensitiveHidden,
    isRevealed,
    isEditingTags,
    tagInput,
    tagColors,
    theme,
    language,
    t,
    compactMode,
    richTextSnapshotPreview,
    dragControls,
    disableLayout,
    setSelectedIndex,
    copyToClipboard,
    setRevealedIds,
    openContent,
    togglePin,
    deleteEntry,
    setEditingTagsId,
    setTagInput,
    handleUpdateTags,
    onQRCode,
    onTransformError,
    onTransformSuccess
}: ClipboardItemRendererProps) => {
    const [detailState, setDetailState] = useState<{ source: ClipboardEntry; value: ClipboardEntry } | null>(null);
    const detail = detailState?.source === item ? detailState.value : null;
    const setDetail = (value: ClipboardEntry | null) => setDetailState(value ? { source: item, value } : null);
    const [resourceState, setResourceState] = useState<{ source: ClipboardEntry; value: ClipboardEntry } | null>(null);
    const resourcePreview = resourceState?.source === item ? resourceState.value : null;
    const setResourcePreview = (value: ClipboardEntry | null) => setResourceState(value ? { source: item, value } : null);
    const mounted = useRef(false);
    const detailGeneration = useRef(0);
    const currentItem = useRef(item);
    currentItem.current = item;
    const sensitive = isSensitiveEntry(item);
    useEffect(() => {
        mounted.current = true;
        const stop = subscribePrivacyInvalidation(() => { detailGeneration.current += 1; setDetail(null); setResourcePreview(null); });
        return () => { mounted.current = false; detailGeneration.current += 1; forgetPrivacyReveal(currentItem.current); stop(); };
    }, []);
    useEffect(() => {
        detailGeneration.current += 1;
        setDetail(null);
        setRevealedIds(previous => {
            if (!previous.has(item.id)) return previous;
            const next = new Set(previous); next.delete(item.id); return next;
        });
    }, [item, setRevealedIds]);
    const loadFullItem = useCallback(() => {
        const requestedItem = currentItem.current;
        const generation = detailGeneration.current;
        const access = beginPrivacyAccess();
        if (!access) return Promise.reject(new Error("PrivacyStateUnavailable"));
        const request = sensitive ? getPrivacyEntry(item.id, item.host_instance_id)
            : invokePrivacyBound<ClipboardEntry>("get_privacy_entry", { id: item.id, expectedHostInstanceId: item.host_instance_id, expectedEpoch: access.epoch, reveal: false });
        return request.then(value => {
            if (!mounted.current || !access.isCurrent() || detailGeneration.current !== generation || currentItem.current !== requestedItem
                || !matchesEntryIdentity(value, item.id, item.host_instance_id)) throw new Error("PrivacyReadSuperseded");
            return value;
        });
    }, [item.id, item.host_instance_id, sensitive]);
    useEffect(() => {
        let disposed = false;
        setResourcePreview(null);
        const access = beginPrivacyAccess();
        if (access && !sensitive && !compactMode && item.content_type === "rich_text" && !item.html_content) {
            void invokePrivacyBound<ClipboardEntry>("get_history_resource_preview", { id: item.id, expectedHostInstanceId: item.host_instance_id, expectedEpoch: access.epoch, maxDimension: 480 })
                .then(value => { if (!disposed && access.isCurrent()) setResourcePreview(value); }).catch(() => undefined);
        }
        return () => { disposed = true; };
    }, [item, compactMode, sensitive]);
    const itemId = item.id;
    const itemContent = item.content;
    const itemContentType = item.content_type;
    const itemTags = item.tags;
    const itemIsPinned = item.is_pinned;

    const onSelect = useCallback(() => {
        setSelectedIndex(index);
    }, [setSelectedIndex, index]);

    const onCopy = useCallback(
        (withFormat?: boolean, pasteImageAsBase64?: boolean) => {
            const access = beginPrivacyAccess();
            if (!access || (sensitive && (!isRevealed || !detail))) {
                onTransformError(item, "privacy_read", t("privacy_reveal_first")); return;
            }
            copyToClipboard(itemId, itemContent, itemContentType, withFormat, pasteImageAsBase64, sensitive ? access.epoch : undefined, undefined, item.host_instance_id);
        },
        [copyToClipboard, itemId, itemContent, itemContentType, sensitive, isRevealed, detail, item, onTransformError, t]
    );

    const onToggleReveal = useCallback(
        (e: MouseEvent) => {
            e.stopPropagation();
            const generation = ++detailGeneration.current;
            if (isRevealed) {
                forgetPrivacyReveal(item);
                clearRichTextSnapshots();
                void hideCompactPreviewGlobal();
                setDetail(null);
                setRevealedIds(previous => { const next = new Set(previous); next.delete(itemId); return next; });
                return;
            }
            const access = beginPrivacyAccess();
            if (!access) return;
            void loadFullItem().then(value => {
                if (!mounted.current || !access.isCurrent() || currentItem.current !== item || detailGeneration.current !== generation) return;
                registerPrivacyReveal(item);
                setDetail(value);
                setRevealedIds(previous => new Set(previous).add(itemId));
            }).catch(() => { if (mounted.current && access.isCurrent() && detailGeneration.current === generation) onTransformError(item, "privacy_read", t("privacy_action_failed")); });
        },
        [setRevealedIds, itemId, isRevealed, item, loadFullItem, onTransformError, t]
    );

    const onOpen = useCallback(
        (e: MouseEvent) => {
            e.stopPropagation();
            const access = beginPrivacyAccess();
            if (!access || (sensitive && (!isRevealed || !detail))) {
                onTransformError(item, "privacy_read", t("privacy_reveal_first")); return;
            }
            openContent(detail ?? item, sensitive ? access.epoch : undefined);
        },
        [openContent, item, detail, sensitive, isRevealed, onTransformError, t]
    );

    const onTogglePin = useCallback(
        (e: MouseEvent, intent?: PrivacyMetadataIntent) => {
            return togglePin(e, itemId, itemIsPinned, item.host_instance_id, intent && { ...intent,
                isCurrent: () => matchesEntryIdentity(currentItem.current, item.id, item.host_instance_id) });
        },
        [togglePin, itemId, itemIsPinned, item]
    );

    const onDelete = useCallback(
        (e: MouseEvent) => {
            deleteEntry(e, itemId, item.host_instance_id);
        },
        [deleteEntry, itemId, item.host_instance_id]
    );

    const onToggleTagEditor = useCallback(
        (e: MouseEvent) => {
            e.stopPropagation();
            if (isEditingTags) {
                setEditingTagsId(null);
            } else {
                setEditingTagsId(itemId);
                setTagInput("");
            }
        },
        [isEditingTags, setEditingTagsId, setTagInput, itemId]
    );

    const onTagAdd = useCallback(async (intent?: PrivacyMetadataIntent) => {
        const newTag = tagInput.trim();
        if (newTag && !itemTags?.includes(newTag)) {
            await handleUpdateTags(itemId, [...(itemTags || []), newTag], item.host_instance_id, intent && { ...intent,
                isCurrent: () => matchesEntryIdentity(currentItem.current, item.id, item.host_instance_id) });
        }
        setTagInput("");
        setEditingTagsId(null);
    }, [tagInput, itemTags, handleUpdateTags, setTagInput, setEditingTagsId, itemId, item]);

    const onTagDelete = useCallback(
        (tag: string, intent?: PrivacyMetadataIntent) => {
            return handleUpdateTags(itemId, itemTags ? itemTags.filter((t) => t !== tag) : [], item.host_instance_id, intent && { ...intent,
                isCurrent: () => matchesEntryIdentity(currentItem.current, item.id, item.host_instance_id) });
        },
        [handleUpdateTags, itemId, itemTags, item]
    );

    const onQRCodeForItem = useCallback(() => {
        if (sensitive && (!isRevealed || !detail)) { onTransformError(item, "privacy_read", t("privacy_reveal_first")); return; }
        const access = beginPrivacyAccess();
        if (!access) return;
        const generation = detailGeneration.current;
        void loadFullItem().then(value => {
            if (mounted.current && access.isCurrent() && currentItem.current === item && detailGeneration.current === generation) onQRCode(value);
        }).catch(() => { if (mounted.current && access.isCurrent() && detailGeneration.current === generation) onTransformError(item, "privacy_read", t("privacy_action_failed")); });
    }, [onQRCode, item, loadFullItem, onTransformError, t, sensitive, isRevealed, detail]);

    const onTransformItemError = useCallback(
        (kind: string, message: string) => {
            onTransformError(item, kind, message);
        },
        [onTransformError, item]
    );

    const onTransformItemSuccess = useCallback(
        (kind: string) => {
            onTransformSuccess(item, kind);
        },
        [onTransformSuccess, item]
    );

    return (
        <ClipboardItem
            id={`clipboard-item-${itemId}`}
            item={isRevealed && detail ? detail : sensitive ? concealSensitiveEntry(item) : resourcePreview ?? item}
            isSelected={isSelected}
            windowPinned={windowPinned}
            isSensitiveHidden={isSensitiveHidden || (sensitive && !detail)}
            isRevealed={isRevealed && !!detail}
            isEditingTags={isEditingTags}
            tagInput={tagInput}
            tagColors={tagColors}
            theme={theme}
            language={language}
            t={t}
            compactMode={compactMode}
            richTextSnapshotPreview={richTextSnapshotPreview}
            onSelect={onSelect}
            onCopy={onCopy}
            onToggleReveal={onToggleReveal}
            onOpen={onOpen}
            onTogglePin={onTogglePin}
            onDelete={onDelete}
            onToggleTagEditor={onToggleTagEditor}
            onTagInput={setTagInput}
            onTagAdd={onTagAdd}
            onTagDelete={onTagDelete}
            dragControls={dragControls}
            disableLayout={disableLayout}
            onQRCode={onQRCodeForItem}
            onTransformItemError={onTransformItemError}
            onTransformItemSuccess={onTransformItemSuccess}
            loadFullItem={loadFullItem}
        />
    );
};

export default memo(ClipboardItemRenderer);
