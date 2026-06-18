import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

/** The 11 content kinds matching Task 27's backend definition */
export const CONTENT_KINDS = [
  { id: "url", label: "链接" },
  { id: "email", label: "邮箱" },
  { id: "phone", label: "电话" },
  { id: "idcard", label: "身份证" },
  { id: "ipv4", label: "IPv4" },
  { id: "ipv6", label: "IPv6" },
  { id: "jwt", label: "JWT" },
  { id: "path", label: "路径" },
  { id: "color_hex", label: "颜色(hex)" },
  { id: "color_rgb", label: "颜色(rgb)" },
  { id: "json", label: "JSON" },
] as const;

export const CONTENT_KIND_IDS = CONTENT_KINDS.map((k) => k.id);

interface UseFilterChipsReturn {
  activeKinds: string[];
  toggle: (kind: string) => void;
  set: (kinds: string[]) => void;
  clear: () => void;
  setEnabled: (enabled: boolean) => void;
  enabled: boolean;
}

/**
 * Manages active filter chip state for content kind filtering.
 * Persists hidden kinds (complement of activeKinds) to settings
 * via `get_hidden_filter_chips` / `set_hidden_filter_chips` Tauri commands.
 */
export const useFilterChips = (): UseFilterChipsReturn => {
  const [activeKinds, setActiveKinds] = useState<string[]>(CONTENT_KIND_IDS);
  const [enabled, setEnabled] = useState(true);
  const loadedRef = useRef(false);

  useEffect(() => {
    invoke<string>("get_hidden_filter_chips")
      .then((csv) => {
        const hidden = csv ? csv.split(",").filter(Boolean) : [];
        setActiveKinds(CONTENT_KIND_IDS.filter((k) => !hidden.includes(k)));
        loadedRef.current = true;
      })
      .catch(() => {
        loadedRef.current = true;
      });
  }, []);

  useEffect(() => {
    if (!loadedRef.current) return;
    const hidden = CONTENT_KIND_IDS.filter((k) => !activeKinds.includes(k));
    invoke("set_hidden_filter_chips", { kinds: hidden.join(",") }).catch(
      () => {}
    );
  }, [activeKinds]);

  const toggle = useCallback((kind: string) => {
    setActiveKinds((prev) =>
      prev.includes(kind) ? prev.filter((k) => k !== kind) : [...prev, kind]
    );
  }, []);

  const set = useCallback((kinds: string[]) => {
    setActiveKinds(kinds);
  }, []);

  const clear = useCallback(() => {
    setActiveKinds([]);
  }, []);

  const setEnabledAction = useCallback((value: boolean) => {
    setEnabled(value);
  }, []);

  return {
    activeKinds,
    toggle,
    set,
    clear,
    setEnabled: setEnabledAction,
    enabled,
  };
};
