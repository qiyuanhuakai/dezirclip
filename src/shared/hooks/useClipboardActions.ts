import { usePlatformCapabilities } from "./usePlatformCapabilities";
import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Dispatch, RefObject, SetStateAction } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import type { ClipboardEntry } from "../types";
import { entryIdentityArgs, entryInstanceArgs, matchesEntryIdentity } from "../lib/historyIdentity";
import type { VirtualClipboardListHandle } from "../../features/clipboard/types";
import { ask } from "@tauri-apps/plugin-dialog";
import { getPrivacyViewState, refreshPrivacyAfterConfirmation } from "../lib/privacySession";
import { capturePrivacyMetadataIntent, needsMetadataPersistenceConfirmation, privacyMetadataWriteArgs } from "../lib/privacyMetadataIntent";
import type { PrivacyMetadataIntent } from "../lib/privacyMetadataIntent";

async function authorizeMetadataWrite(id: number, persists: boolean, t: (key: string) => string, original?: PrivacyMetadataIntent) {
  const intent = original ?? capturePrivacyMetadataIntent(getPrivacyViewState());
  if (!intent) throw new Error("PrivacyStateUnavailable");
  let confirmed = false;
  if (needsMetadataPersistenceConfirmation(intent, id, persists)) {
    confirmed = await ask(t("privacy_metadata_persist_confirm"), { title: t("privacy_metadata_persist_title"), kind: "warning" });
    await refreshPrivacyAfterConfirmation(confirmed ? intent.epoch : undefined);
    if (!confirmed) return null;
  }
  return privacyMetadataWriteArgs(intent, getPrivacyViewState(), confirmed);
}

interface UseClipboardActionsOptions {
  t: (key: string) => string;
  pushToast: (msg: string, duration?: number) => number;
  deleteAfterPaste: boolean;
  moveToTopAfterPaste: boolean;
  setSearch: (val: string) => void;
  setHistory: Dispatch<SetStateAction<ClipboardEntry[]>>;
  virtualListRef: RefObject<VirtualClipboardListHandle | null>;
}

export const useClipboardActions = ({
  t,
  pushToast,
  deleteAfterPaste,
  moveToTopAfterPaste,
  setSearch,
  setHistory,
  virtualListRef
}: UseClipboardActionsOptions) => {
  const { report, pending, failed } = usePlatformCapabilities();
  const manualCopy = report?.native_wayland === true;
  const copyToClipboard = useCallback(
    async (
      id: number,
      content: string,
      contentType: string,
      pasteWithFormat = false,
      pasteImageAsBase64 = false,
      expectedPrivacyEpoch?: number,
      derivedFromId?: number,
      hostInstanceId?: string
    ) => {
      try {
        if (pending || failed) { pushToast(t("platform_capabilities_unavailable"), 3000); return; }
        if (report?.write.availability.status !== "available") { pushToast(t("wayland_unavailable"), 3000); return; }
        if (document.activeElement instanceof HTMLElement) {
          document.activeElement.blur();
        }

        await invoke("copy_to_clipboard", {
          content,
          contentType,
          paste: !manualCopy,
          id: id,
          deleteAfterUse: !manualCopy && deleteAfterPaste,
          pasteWithFormat,
          pasteImageAsBase64,
          expectedPrivacyEpoch,
          derivedFromId,
          ...entryInstanceArgs(hostInstanceId),
          moveToTop: !manualCopy && moveToTopAfterPaste
        });

        if (!manualCopy && moveToTopAfterPaste && !deleteAfterPaste) {
          const now = Date.now();
          setHistory((prev) =>
            prev.map((item) =>
              matchesEntryIdentity(item, id, hostInstanceId) ? { ...item, timestamp: now } : item
            )
          );
        }

        if (manualCopy) pushToast(t("wayland_manual_copy_done"));
        else setSearch("");
      } catch (err) {
        const errorMsg = t("copy_failed") + (err?.toString() || "");
        pushToast(errorMsg, 3000);
      }
    },
    [deleteAfterPaste, moveToTopAfterPaste, pushToast, setHistory, setSearch, t, manualCopy, pending, failed, report]
  );

  const openContent = useCallback(
    async (item: ClipboardEntry, expectedPrivacyEpoch?: number) => {
      try {
        await invoke("open_content", {
          ...entryIdentityArgs(item),
          content: item.content,
          contentType: item.content_type,
          expectedPrivacyEpoch
        });
      } catch (err) {
        const errorMsg = t("open_failed") + (err?.toString() || "");
        pushToast(errorMsg, 3000);
      }
    },
    [pushToast, t]
  );

  const deleteEntry = useCallback(
    async (e: ReactMouseEvent, id: number, hostInstanceId?: string) => {
      e.stopPropagation();
      try {
        await invoke("delete_clipboard_entry", { id, ...entryInstanceArgs(hostInstanceId) });
        setHistory((prev) => prev.filter((item) => !matchesEntryIdentity(item, id, hostInstanceId)));
      } catch (err) {
        const errorMsg = "删除失败: " + (err?.toString() || "");
        pushToast(errorMsg, 3000);
      }
    },
    [pushToast, setHistory]
  );

  const togglePin = useCallback(
    async (e: ReactMouseEvent, id: number, currentPinned: boolean, hostInstanceId?: string, intent?: PrivacyMetadataIntent) => {
      e.stopPropagation();
      try {
        const authorization = await authorizeMetadataWrite(id, !currentPinned, t, intent);
        if (!authorization) return;
        await invoke("toggle_clipboard_pin", { id, isPinned: !currentPinned, ...entryInstanceArgs(hostInstanceId), ...authorization });
        setHistory((prev) =>
          prev
            .map((item) =>
              matchesEntryIdentity(item, id, hostInstanceId) ? { ...item, is_pinned: !currentPinned } : item
            )
            .sort((a, b) => {
              if (a.is_pinned === b.is_pinned) return b.timestamp - a.timestamp;
              return a.is_pinned ? -1 : 1;
            })
        );
      } catch (err) {
        const errorMsg =
          (currentPinned ? "取消固定失败" : "固定失败") + ": " + (err?.toString() || "");
        pushToast(errorMsg, 3000);
      }
    },
    [pushToast, setHistory, t]
  );

  const handleUpdateTags = useCallback(
    async (id: number, newTags: string[], hostInstanceId?: string, intent?: PrivacyMetadataIntent) => {
      try {
        const authorization = await authorizeMetadataWrite(id, true, t, intent);
        if (!authorization) return;
        const newId = await invoke<number>("update_tags", { id, tags: newTags, ...entryInstanceArgs(hostInstanceId), ...authorization });
        setHistory((prev) =>
          prev.map((item) => (matchesEntryIdentity(item, id, hostInstanceId) ? { ...item, id: newId, tags: newTags } : item))
        );

        setTimeout(() => {
          if (virtualListRef.current) {
            virtualListRef.current.resetAfterIndex(0);
          }
        }, 0);
      } catch (err) {
        pushToast(t("privacy_action_failed") + ": " + String(err), 3000);
      }
    },
    [setHistory, virtualListRef, pushToast, t]
  );

  return {
    copyToClipboard,
    openContent,
    deleteEntry,
    togglePin,
    handleUpdateTags
  };
};


