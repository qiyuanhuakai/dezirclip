import { memo, useCallback } from "react";
import type { Dispatch, MouseEvent, SetStateAction } from "react";
import type { DragControls } from "framer-motion";
import ClipboardItem from "./ClipboardItem";
import type { ClipboardEntry, Locale } from "../../../shared/types";

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
        pasteImageAsBase64?: boolean
    ) => Promise<void>;
    setRevealedIds: Dispatch<SetStateAction<Set<number>>>;
    openContent: (item: ClipboardEntry) => void;
    togglePin: (event: MouseEvent, id: number, isPinned: boolean) => void;
    deleteEntry: (event: MouseEvent, id: number) => void;
    setEditingTagsId: Dispatch<SetStateAction<number | null>>;
    setTagInput: Dispatch<SetStateAction<string>>;
    handleUpdateTags: (id: number, tags: string[]) => void;
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
            copyToClipboard(itemId, itemContent, itemContentType, withFormat, pasteImageAsBase64);
        },
        [copyToClipboard, itemId, itemContent, itemContentType]
    );

    const onToggleReveal = useCallback(
        (e: MouseEvent) => {
            e.stopPropagation();
            setRevealedIds((prev) => {
                const next = new Set(prev);
                if (next.has(itemId)) next.delete(itemId);
                else next.add(itemId);
                return next;
            });
        },
        [setRevealedIds, itemId]
    );

    const onOpen = useCallback(
        (e: MouseEvent) => {
            e.stopPropagation();
            openContent(item);
        },
        [openContent, item]
    );

    const onTogglePin = useCallback(
        (e: MouseEvent) => {
            togglePin(e, itemId, itemIsPinned);
        },
        [togglePin, itemId, itemIsPinned]
    );

    const onDelete = useCallback(
        (e: MouseEvent) => {
            deleteEntry(e, itemId);
        },
        [deleteEntry, itemId]
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

    const onTagAdd = useCallback(() => {
        const newTag = tagInput.trim();
        if (newTag && !itemTags?.includes(newTag)) {
            handleUpdateTags(itemId, [...(itemTags || []), newTag]);
        }
        setTagInput("");
        setEditingTagsId(null);
    }, [tagInput, itemTags, handleUpdateTags, setTagInput, setEditingTagsId, itemId]);

    const onTagDelete = useCallback(
        (tag: string) => {
            handleUpdateTags(itemId, itemTags ? itemTags.filter((t) => t !== tag) : []);
        },
        [handleUpdateTags, itemId, itemTags]
    );

    const onQRCodeForItem = useCallback(() => {
        onQRCode(item);
    }, [onQRCode, item]);

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
            item={item}
            isSelected={isSelected}
            windowPinned={windowPinned}
            isSensitiveHidden={isSensitiveHidden}
            isRevealed={isRevealed}
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
        />
    );
};

export default memo(ClipboardItemRenderer);
