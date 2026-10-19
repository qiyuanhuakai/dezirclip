import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const previewPath = (content: string | null): string | null => {
  if (!content) return null;
  let path = content.trim();
  if (/^file:/i.test(path)) {
    try {
      const url = new URL(path);
      path = decodeURIComponent(url.pathname);
      if (url.hostname && url.hostname !== "localhost") {
        path = `//${url.hostname}${path}`;
      } else if (/^\/[a-zA-Z]:\//.test(path)) {
        path = path.slice(1);
      }
    } catch {
      return null;
    }
  }
  return /^(?:[a-zA-Z]:[\\/]|\/|\\\\)/.test(path) && /\.(?:png|jpe?g|bmp)$/i.test(path)
    ? path
    : null;
};

interface PreviewResult {
  path: string;
  revision: number;
  src: string | null;
}

export const useImagePreview = (content: string | null, revision: number) => {
  const path = previewPath(content);
  const [result, setResult] = useState<PreviewResult | null>(null);

  useEffect(() => {
    if (!path) {
      setResult(null);
      return;
    }
    let cancelled = false;
    invoke<string>("get_image_preview", { path })
      .then((src) => {
        if (!cancelled) {
          setResult({
            path,
            revision,
            src: src.startsWith("data:image/png;base64,") ? src : null
          });
        }
      })
      .catch(() => {
        if (!cancelled) setResult({ path, revision, src: null });
      });
    return () => {
      cancelled = true;
    };
  }, [path, revision]);

  const current = result !== null && result.path === path && result.revision === revision;
  return {
    src: current ? result.src : null,
    pending: path !== null && !current,
    fallback: () => {
      if (path) setResult({ path, revision, src: null });
    }
  };
};
