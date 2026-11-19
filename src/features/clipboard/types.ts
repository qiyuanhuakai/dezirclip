import type { MouseEvent, ReactNode } from "react";
import type { DragControls } from "framer-motion";
import type { ClipboardEntry, Locale } from "../../shared/types";
import type { PrivacyMetadataIntent } from "../../shared/lib/privacyMetadataIntent";

export interface ClipboardItemProps {
  item: ClipboardEntry;
  isSelected: boolean;
  windowPinned: boolean;
  isSensitiveHidden: boolean;
  isRevealed: boolean;
  isEditingTags: boolean;
  tagInput: string;
  theme: string;
  language: Locale;
  t: (key: string) => string;
  tagColors?: Record<string, string>;
  richTextSnapshotPreview?: boolean;
  previewFromKeyboard?: boolean;

  onSelect: () => void;
  onCopy: (withFormat?: boolean, pasteImageAsBase64?: boolean) => void;
  onToggleReveal: (e: MouseEvent) => void;
  onOpen: (e: MouseEvent) => void;
  onTogglePin: (e: MouseEvent, intent?: PrivacyMetadataIntent) => void | Promise<void>;
  onDelete: (e: MouseEvent) => void;
  onToggleTagEditor: (e: MouseEvent) => void;
  onTagInput: (val: string) => void;
  onTagAdd: (intent?: PrivacyMetadataIntent) => void | Promise<void>;
  onTagDelete: (tag: string, intent?: PrivacyMetadataIntent) => void | Promise<void>;
  dragControls?: DragControls;
  id?: string;
  disableLayout?: boolean;
}

export type ClipboardRenderItem = (
  item: ClipboardEntry,
  index: number,
  isFirst: boolean
) => ReactNode;

import type { VirtualClipboardListProps as ListProps } from "@deziro/ui-react/components/VirtualClipboardList";
export type { VirtualClipboardListHandle } from "@deziro/ui-react/components/VirtualClipboardList";
export type VirtualClipboardListProps = ListProps<ClipboardEntry>;
