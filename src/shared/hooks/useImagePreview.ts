import { useEffect, useState } from "react";
import type { ClipboardEntry } from "../types";
import { usePrivacySession } from "./usePrivacySession";
import { invokePrivacyBound } from "../lib/privacySession";
import { isRamImageSource } from "../lib/privacyRuntime";

interface PreviewResult {
  id: number;
  revision: number;
  generation: number;
  hostInstanceId?: string;
  src: string | null;
}

export const useImagePreview = (id: number, content: string | null, revision: number, enabled: boolean, hostInstanceId?: string) => {
  const privacy = usePrivacySession();
  const [result, setResult] = useState<PreviewResult | null>(null);
  const inline = enabled && privacy.phase === "ready" && content && isRamImageSource(content) ? content : null;
  const epoch = privacy.snapshot?.privacy_epoch;
  const needsRead = enabled && !inline && id !== 0 && privacy.phase === "ready" && epoch !== undefined;
  useEffect(() => {
    let cancelled = false;
    setResult(null);
    if (needsRead) {
      void invokePrivacyBound<ClipboardEntry>("get_history_resource_preview", { id, expectedEpoch: epoch, maxDimension: 480, expectedHostInstanceId: hostInstanceId })
        .then(entry => {
          if (!cancelled) setResult({ id, revision, hostInstanceId, generation: privacy.generation,
            src: isRamImageSource(entry.content) ? entry.content : null });
        }).catch(() => {
          if (!cancelled) setResult({ id, revision, hostInstanceId, generation: privacy.generation, src: null });
        });
    }
    return () => {
      cancelled = true;
    };
  }, [id, revision, needsRead, epoch, privacy.generation, hostInstanceId]);

  const current = enabled && privacy.phase === "ready" && result?.id === id
    && result.hostInstanceId === hostInstanceId && result.revision === revision && result.generation === privacy.generation;
  return {
    src: inline || (current ? result.src : null),
    pending: needsRead && !current,
  };
};
