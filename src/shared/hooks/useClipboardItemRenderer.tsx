import { useCallback } from "react";
import type { Dispatch, SetStateAction, MouseEvent, ReactNode } from "react";
import type { DragControls } from "framer-motion";
import ClipboardItemRenderer from "../../features/clipboard/components/ClipboardItemRenderer";
import type { ClipboardEntry } from "../types";
import type { Locale } from "../types";

interface UseClipboardItemRendererOptions {
  privacyProtection: boolean;
  revealedIds: Set<number>;
  isKeyboardMode: boolean;
  selectedIndex: number;
  isWindowPinned: boolean;
  editingTagsId: number | null;
  tagInput: string;
  tagColors: Record<string, string>;
  theme: string;
  language: Locale;
  t: (key: string) => string;
  compactMode: boolean;
  richTextSnapshotPreview: boolean;
  copyToClipboard: (
    id: number,
    content: string,
    contentType: string,
    pasteWithFormat?: boolean,
    pasteImageAsBase64?: boolean
  ) => Promise<void>;
  setSelectedIndex: Dispatch<SetStateAction<number>>;
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

type RenderItemContent = (
  item: ClipboardEntry,
  index: number,
  dragControls?: DragControls,
  disableLayout?: boolean
) => ReactNode;

export const useClipboardItemRenderer = ({
  privacyProtection,
  revealedIds,
  isKeyboardMode,
  selectedIndex,
  isWindowPinned,
  editingTagsId,
  tagInput,
  tagColors,
  theme,
  language,
  t,
  compactMode,
  richTextSnapshotPreview,
  copyToClipboard,
  setSelectedIndex,
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
}: UseClipboardItemRendererOptions): { renderItemContent: RenderItemContent } => {
  const renderItemContent = useCallback(
    (item: ClipboardEntry, index: number, dragControls?: DragControls, disableLayout?: boolean) => {
      const isSensitiveHidden =
        privacyProtection &&
        (item.tags?.includes("sensitive") ||
          item.tags?.includes("密码") ||
          item.tags?.includes("password")) &&
        !revealedIds.has(item.id);
      const isEditingTags = editingTagsId === item.id;

      return (
        <ClipboardItemRenderer
          item={item}
          index={index}
          isSelected={isKeyboardMode && index === selectedIndex}
          windowPinned={isWindowPinned}
          isSensitiveHidden={!!isSensitiveHidden}
          isRevealed={revealedIds.has(item.id)}
          isEditingTags={isEditingTags}
          tagInput={isEditingTags ? tagInput : ""}
          tagColors={tagColors}
          theme={theme}
          language={language}
          t={t}
          compactMode={compactMode}
          richTextSnapshotPreview={richTextSnapshotPreview}
          dragControls={dragControls}
          disableLayout={disableLayout}
          setSelectedIndex={setSelectedIndex}
          copyToClipboard={copyToClipboard}
          setRevealedIds={setRevealedIds}
          openContent={openContent}
          togglePin={togglePin}
          deleteEntry={deleteEntry}
          setEditingTagsId={setEditingTagsId}
          setTagInput={setTagInput}
          handleUpdateTags={handleUpdateTags}
          onQRCode={onQRCode}
          onTransformError={onTransformError}
          onTransformSuccess={onTransformSuccess}
        />
      );
    },
    [
      privacyProtection,
      revealedIds,
      isKeyboardMode,
      selectedIndex,
      isWindowPinned,
      editingTagsId,
      tagInput,
      tagColors,
      theme,
      language,
      t,
      compactMode,
      richTextSnapshotPreview,
      copyToClipboard,
      setSelectedIndex,
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
    ]
  );

  return { renderItemContent };
};

