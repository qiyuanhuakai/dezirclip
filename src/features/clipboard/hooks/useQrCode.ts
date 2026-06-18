import { useCallback, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface QrCodeResult {
  dataUrl: string | null;
  loading: boolean;
  error: string | null;
  generate: (content: string, size: number) => Promise<void>;
}

interface CacheEntry {
  dataUrl: string;
  key: string;
}

export const useQrCode = (): QrCodeResult => {
  const [dataUrl, setDataUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cacheRef = useRef<CacheEntry | null>(null);
  const generationIdRef = useRef(0);

  const generate = useCallback(async (content: string, size: number) => {
    if (!content) {
      setError("内容不能为空");
      setDataUrl(null);
      return;
    }

    const cacheKey = `${content}:${size}`;
    if (cacheRef.current?.key === cacheKey) {
      setDataUrl(cacheRef.current.dataUrl);
      setError(null);
      return;
    }

    const id = ++generationIdRef.current;
    setLoading(true);
    setError(null);

    try {
      const result = await invoke<string>("generate_qr_png", {
        content,
        sizePx: size,
      });

      // Stale check — if a newer generation started, discard this result
      if (id !== generationIdRef.current) return;

      cacheRef.current = { dataUrl: result, key: cacheKey };
      setDataUrl(result);
      setError(null);
    } catch (err) {
      if (id !== generationIdRef.current) return;
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg);
      setDataUrl(null);
    } finally {
      if (id === generationIdRef.current) {
        setLoading(false);
      }
    }
  }, []);

  return { dataUrl, loading, error, generate };
};
