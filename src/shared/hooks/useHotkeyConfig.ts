import { normalizeShortcutForCompare } from "../lib/shortcutKeys";
import { useCallback, useEffect, useRef } from "react";
import { invoke } from "../lib/desktopInvoke";

type HotkeyMode = "main" | "sequential" | "rich" | "search";

interface UseHotkeyConfigOptions {
  hotkey: string;
  setHotkey: (val: string) => void;
  sequentialHotkey: string;
  setSequentialHotkey: (val: string) => void;
  richPasteHotkey: string;
  setRichPasteHotkey: (val: string) => void;
  searchHotkey: string;
  setSearchHotkey: (val: string) => void;
  sequentialMode: boolean;
  setIsRecording: (val: boolean) => void;
  saveAppSetting: (type: string, value: string) => void;
  t: (key: string) => string;
  pushToast: (msg: string, duration?: number) => number;
}

export const useHotkeyConfig = ({
  hotkey,
  setHotkey,
  sequentialHotkey,
  setSequentialHotkey,
  richPasteHotkey,
  setRichPasteHotkey,
  searchHotkey,
  setSearchHotkey,
  sequentialMode,
  setIsRecording,
  saveAppSetting,
  t,
  pushToast
}: UseHotkeyConfigOptions) => {
  // A Win+V transaction can add/remove and then roll back before its original
  // async handler receives new props. Always mutate the last confirmed list.
  const confirmedHotkeys = useRef(hotkey);
  useEffect(() => { confirmedHotkeys.current = hotkey; }, [hotkey]);
  const parseMainHotkeys = useCallback(
    (value: string): string[] =>
      value
        .split(/[\r\n]+/g)
        .map((item) => item.trim())
        .filter((item) => !!item),
    []
  );

  const serializeMainHotkeys = useCallback(
    (items: string[]): string =>
      items
        .map((item) => item.trim())
        .filter((item) => !!item)
        .join("\n"),
    []
  );

  const persistMainHotkeys = useCallback(
    async (items: string[]) => {
      const serialized = serializeMainHotkeys(items);
      if (import.meta.env.MODE === "mygo") {
        await invoke("register_hotkey", { hotkey: serialized });
        confirmedHotkeys.current = serialized;
        setHotkey(serialized);
        return;
      }
      confirmedHotkeys.current = serialized;
      setHotkey(serialized);
      saveAppSetting("hotkey", serialized);
      await invoke("register_hotkey", { hotkey: serialized }).catch((err) => {
        if (serialized) {
          const errorMsg = t("hotkey_register_failed") + (err?.toString() || "");
          pushToast(errorMsg, 3000);
        }
      });
    },
    [pushToast, saveAppSetting, serializeMainHotkeys, setHotkey, t]
  );

  const resolveHotkeyErrorMessage = useCallback(
    (err: unknown): string => {
      if (typeof err === "string" && err.trim()) return err;
      if (err instanceof Error && err.message.trim()) return err.message;
      const fallback = t("hotkey_unavailable");
      if (err === null || err === undefined) return fallback;
      const text = String(err).trim();
      return text || fallback;
    },
    [t]
  );

  const checkHotkeyConflict = useCallback(
    (newHotkey: string, mode: HotkeyMode): boolean => {
      if (!newHotkey) return false;
      const normalizedNew = normalizeShortcutForCompare(newHotkey);
      const mainHotkeys = parseMainHotkeys(hotkey);
      const normalizedMain = new Set(mainHotkeys.map(normalizeShortcutForCompare));
      const normalizedSequential = normalizeShortcutForCompare(sequentialHotkey);
      const normalizedRich = normalizeShortcutForCompare(richPasteHotkey);
      const normalizedSearch = normalizeShortcutForCompare(searchHotkey);

      const conflicts = [];
      if (mode !== "main" && normalizedMain.has(normalizedNew)) conflicts.push(t("global_hotkey"));
      if (mode !== "sequential" && sequentialMode && normalizedNew === normalizedSequential) {
        conflicts.push(t("sequential_paste_hotkey_label"));
      }
      if (mode !== "rich" && normalizedNew === normalizedRich) {
        conflicts.push(t("rich_paste_hotkey_label"));
      }
      if (mode !== "search" && normalizedNew === normalizedSearch) {
        conflicts.push(t("search_hotkey_label"));
      }

      if (conflicts.length > 0) {
        const msg = t("hotkey_conflict_toast").replace("{name}", conflicts[0]);
        pushToast(msg, 5000);
        return true;
      }
      return false;
    },
    [hotkey, parseMainHotkeys, sequentialMode, sequentialHotkey, richPasteHotkey, searchHotkey, t, pushToast]
  );

  const updateHotkey = useCallback(
    async (newHotkey: string) => {
      const hasConflict = checkHotkeyConflict(newHotkey, "main");
      if (hasConflict) {
        setIsRecording(false);
        return;
      }

      if (newHotkey) {
        try {
          await invoke<boolean>("test_hotkey_available", { hotkey: newHotkey });
        } catch (err) {
          const errorMsg = `${newHotkey}: ${resolveHotkeyErrorMessage(err)}`;
          pushToast(errorMsg, 5000);
          setIsRecording(false);
          return;
        }
      }

      await persistMainHotkeys(newHotkey ? [newHotkey] : []);
      setIsRecording(false);
    },
    [checkHotkeyConflict, persistMainHotkeys, pushToast, resolveHotkeyErrorMessage, setIsRecording]
  );

  const addMainHotkey = useCallback(
    async (newHotkey: string, options?: { skipAvailabilityCheck?: boolean }) => {
      const value = newHotkey.trim();
      if (!value) return false;

      const hasConflict = checkHotkeyConflict(value, "main");
      if (hasConflict) {
        setIsRecording(false);
        return false;
      }

      if (!options?.skipAvailabilityCheck) {
        try {
          await invoke<boolean>("test_hotkey_available", { hotkey: value });
        } catch (err) {
          const errorMsg = `${value}: ${resolveHotkeyErrorMessage(err)}`;
          setIsRecording(false);
          throw new Error(errorMsg);
        }
      }

      const existing = parseMainHotkeys(confirmedHotkeys.current);
      const normalizedValue = normalizeShortcutForCompare(value);
      const hasExisting = existing.some((item) => normalizeShortcutForCompare(item) === normalizedValue);
      if (hasExisting) {
        setIsRecording(false);
        return true;
      }

      await persistMainHotkeys([...existing, value]);
      setIsRecording(false);
      return true;
    },
    [checkHotkeyConflict, hotkey, parseMainHotkeys, persistMainHotkeys, pushToast, resolveHotkeyErrorMessage, setIsRecording]
  );

  const removeMainHotkey = useCallback(
    async (targetHotkey: string) => {
      const existing = parseMainHotkeys(confirmedHotkeys.current);
      const normalizedTarget = normalizeShortcutForCompare(targetHotkey);
      const next = existing.filter((item) => normalizeShortcutForCompare(item) !== normalizedTarget);
      if (next.length === existing.length) return false;
      await persistMainHotkeys(next);
      return true;
    },
    [hotkey, parseMainHotkeys, persistMainHotkeys]
  );

  const saveActionHotkey = useCallback(async (
    mode: Exclude<HotkeyMode, "main">,
    value: string,
    previous: string,
    command: "set_sequential_hotkey" | "set_rich_paste_hotkey" | "set_search_hotkey",
    apply: (value: string) => void,
  ) => {
    if (normalizeShortcutForCompare(value) === normalizeShortcutForCompare(previous) || checkHotkeyConflict(value, mode)) return;
    try {
      if (value && !await invoke<boolean>("test_hotkey_available", { hotkey: value })) {
        pushToast(t("hotkey_unavailable"), 5000);
        return;
      }
      await invoke(command, { hotkey: value });
      apply(value);
    } catch (error) {
      pushToast(t("hotkey_register_failed") + resolveHotkeyErrorMessage(error), 5000);
    }
  }, [checkHotkeyConflict, pushToast, resolveHotkeyErrorMessage, t]);

  const updateSequentialHotkey = useCallback((value: string) =>
    saveActionHotkey("sequential", value, sequentialHotkey, "set_sequential_hotkey", setSequentialHotkey),
  [saveActionHotkey, sequentialHotkey, setSequentialHotkey]);
  const updateRichPasteHotkey = useCallback((value: string) =>
    saveActionHotkey("rich", value, richPasteHotkey, "set_rich_paste_hotkey", setRichPasteHotkey),
  [richPasteHotkey, saveActionHotkey, setRichPasteHotkey]);
  const updateSearchHotkey = useCallback((value: string) =>
    saveActionHotkey("search", value, searchHotkey, "set_search_hotkey", setSearchHotkey),
  [saveActionHotkey, searchHotkey, setSearchHotkey]);

  return {
    checkHotkeyConflict,
    updateHotkey,
    addMainHotkey,
    removeMainHotkey,
    updateSequentialHotkey,
    updateRichPasteHotkey,
    updateSearchHotkey
  };
};
