import { useEffect, useState } from "react";
import type { ClipboardEntry } from "../types";
import { usePrivacySession } from "./usePrivacySession";
import { invokePrivacyBound, privacyListGeneration } from "../lib/privacySession";
import { isRamImageSource } from "../lib/privacyRuntime";
import { schedulePreview } from "../lib/previewQueue";

interface PreviewResult {
  id: number;
  revision: number;
  generation: number;
  hostInstanceId?: string;
  src: string | null;
}

export const useImagePreview = (id: number, content: string | null, revision: number, enabled: boolean, hostInstanceId?: string) => {
  const privacy = usePrivacySession();
  const generation = privacyListGeneration();
  const [result, setResult] = useState<PreviewResult | null>(null);
  const inline = enabled && privacy.phase === "ready" && content && isRamImageSource(content) ? content : null;
  const epoch = privacy.snapshot?.privacy_epoch;
  const needsRead = enabled && !inline && id !== 0 && privacy.phase === "ready" && epoch !== undefined;
  useEffect(() => {
    const controller = new AbortController();
    setResult(null);
    if (needsRead) {
      void schedulePreview(controller.signal, () => invokePrivacyBound<ClipboardEntry>("get_history_resource_preview", { id, expectedEpoch: epoch, maxDimension: 480, expectedHostInstanceId: hostInstanceId }))
        .then(entry => {
          if (!controller.signal.aborted) setResult({ id, revision, hostInstanceId, generation,
            src: isRamImageSource(entry.content) ? entry.content : null });
        }).catch(() => {
          if (!controller.signal.aborted) setResult({ id, revision, hostInstanceId, generation, src: null });
        });
    }
    return () => {
      controller.abort();
    };
  }, [id, revision, needsRead, epoch, generation, hostInstanceId]);

  const current = enabled && privacy.phase === "ready" && result?.id === id
    && result.hostInstanceId === hostInstanceId && result.revision === revision && result.generation === generation;
  return {
    src: inline || (current ? result.src : null),
    pending: needsRead && !current,
  };
};
