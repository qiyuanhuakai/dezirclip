import { useState } from "react";
import { CaptureEngineStatus, CaptureSettingsActions, CaptureSettingsHeading, FixedCaptureRenderer } from "../src";

export function CaptureActionsFixture({ state }: { state: string }) {
  const [action, setAction] = useState("");
  return <div className="settings-view showcase-capture-config" style={{ width: state === "narrow" ? 240 : 500 }}>
    <CaptureSettingsHeading label="Capture settings" />
    <CaptureSettingsActions label="Capture now" region="Capture region" full="Capture full screen" disabled={state === "disabled"}
      onRegion={() => setAction("Region selected")} onFull={() => setAction("Full screen selected")} />
    {action && <span role="status">{action}</span>}
  </div>;
}

export function CaptureEngineFixture({ state }: { state: string }) {
  const [retried, setRetried] = useState(false);
  const loaded = state === "available" || retried;
  const status = loaded ? "Tesseract (Available)"
    : state === "unavailable" ? "Tesseract (Not available)"
    : state === "failed" ? "Could not check the OCR engine" : "Loading…";
  return <div className="settings-view showcase-capture-config" style={{ width: 430 }}>
    <CaptureEngineStatus label="OCR engine status" status={status} available={loaded}
      retry={state === "loading" ? "Processing…" : "Retry"} disabled={state === "loading"} onRetry={() => setRetried(true)} />
  </div>;
}

export function FixedCaptureRendererFixture() {
  return <div className="settings-view showcase-capture-config" style={{ width: 330 }}>
    <FixedCaptureRenderer label="Quick Paste interface" value="MyGo Native" />
  </div>;
}
