import { useEffect } from "react";
import type { RefObject } from "react";
import type { ClipboardEntry } from "../types";
import type { VirtualClipboardListHandle } from "../../features/clipboard/types";

interface UseScrollToSelectionOptions {
  filteredHistory: ClipboardEntry[];
  selectedIndex: number;
  isKeyboardMode: boolean;
  idPrefix?: string;
  virtualListRef?: RefObject<VirtualClipboardListHandle | null>;
}

export const useScrollToSelection = ({
  filteredHistory,
  selectedIndex,
  isKeyboardMode,
  idPrefix = "clipboard-item-",
  virtualListRef
}: UseScrollToSelectionOptions) => {
  useEffect(() => {
    if (isKeyboardMode && selectedIndex >= 0 && selectedIndex < filteredHistory.length) {
      const item = filteredHistory[selectedIndex];
      if (virtualListRef?.current) {
        virtualListRef.current.scrollToItem(selectedIndex);
        return;
      }

      const el = document.getElementById(`${idPrefix}${item.id}`);
      if (el) {
        el.scrollIntoView({ behavior: "auto", block: "nearest" });
      }
    }
  }, [filteredHistory, selectedIndex, isKeyboardMode, idPrefix, virtualListRef]);
};


