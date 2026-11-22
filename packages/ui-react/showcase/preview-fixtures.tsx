import { useState } from "react";
import { Pin, Tag, X } from "lucide-react";
import { AttachmentPreview, HistoryCard, HistoryCardAction, HistoryCardContent, HistoryCardMeta, HistoryImagePreview, HistoryOCRBadge, HistoryOCRResult, HistoryRichTextPreview, PreviewImage, PreviewResizeHandles, PreviewToolbar, WheelHotZone,
  type PreviewZoom } from "../src";

const filenameList = ["Project.png", "Screenshot.png", "Notes.pdf", "Planning.xlsx", "Desktop.zip", "Summary.txt", "Log.txt"];
function sampleImage(width = 160, height = 96): string {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d")!;
  context.scale(width / 160, height / 96);
  context.fillStyle = "#21365a";
  context.fillRect(0, 0, 160, 96);
  context.fillStyle = "#64a1b1";
  context.fillRect(12, 12, 80, 65);
  context.fillStyle = "#dcc78a";
  context.fillRect(103, 24, 42, 32);
  context.fillStyle = "#e6e8f0";
  context.fillRect(104, 66, 32, 5);
  return canvas.toDataURL("image/png");
}

const picture = sampleImage();
const labels = {
  preview_toolbar_actual: "Actual size", preview_toolbar_close: "Close preview", preview_toolbar_drag: "Move preview",
  preview_toolbar_fit: "Fit image", preview_toolbar_fit_label: "Fit", preview_toolbar_opacity: "Preview opacity",
  preview_toolbar_pin: "Pin preview", preview_toolbar_title: "Content preview", preview_toolbar_zoom: "Image zoom",
  preview_toolbar_zoom_in: "Zoom in", preview_toolbar_zoom_out: "Zoom out",
};

export function PreviewToolbarFixture({ state }: { state: string }) {
  const [pinned, setPinned] = useState(state === "pinned");
  const [opacity, setOpacity] = useState(72);
  const [zoom, setZoom] = useState<PreviewZoom>("fit");
  return <PreviewToolbar pinned={pinned} opacity={opacity} image={pinned} zoom={zoom} labels={labels}
    onPin={setPinned} onClose={() => setPinned(false)} onOpacityChange={setOpacity} onZoomChange={setZoom} />;
}

export function PreviewImageFixture({ state }: { state: string }) {
  const [zoom, setZoom] = useState<PreviewZoom>(state === "zoomed" ? 2 : "fit");
  return <section className="showcase-image compact-preview-root">
    <div>Image preview</div>
    <PreviewImage src={picture} zoom={zoom} onZoomChange={setZoom} onFitZoom={() => undefined} onLoad={() => undefined} />
  </section>;
}

export function AttachmentFixture({ state }: { state: string }) {
  const compact = state.startsWith("compact");
  const expanded = state === "expanded";
  const markup = state.includes("media") || expanded ? filenameList.map((name, index) =>
    `<figure>${index < 2 ? `<img src="${picture}" alt="${name}">` : ""}<figcaption>${name}</figcaption></figure>`).join("") : null;
  return <div style={{ width: state.includes("narrow") ? 220 : 480 }}><HistoryCard compact={compact} disableLayout data-attachment-card>
    <HistoryCardContent>
      <AttachmentPreview names={filenameList.join("\n")} html={markup}
        fileLabel="Attachments" expanded={expanded} compact={compact} />
    </HistoryCardContent>
  </HistoryCard></div>;
}

const historyPictures = [sampleImage(32, 24), picture, sampleImage(72, 230)];
const historyPictureLabels = ["Small image", "Landscape image", "Portrait image"];

export function HistoryOCRFixture({ state }: { state: string }) {
  const [expanded, setExpanded] = useState(false);
  const status = state === "processing" || state === "failed" || state === "unsupported" ? state : "done";
  const label = { processing: "Recognizing text…", done: "OCR text", failed: "OCR failed", unsupported: "OCR unavailable" }[status];
  return <div style={{ width: 330 }}><HistoryCard compact={state === "compact-expanded"} disableLayout>
    <HistoryCardMeta source="Editor" time="12:30" status={<HistoryOCRBadge status={status}>{label}</HistoryOCRBadge>}
      actions={<>
        <HistoryCardAction title="Pin"><Pin size={12} /></HistoryCardAction>
        <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
        <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
      </>} />
    <HistoryCardContent>
      <HistoryImagePreview alt="Invoice image" source={picture} />
    </HistoryCardContent>
    {status === "done" && <HistoryOCRResult label={label} toggleLabel="Toggle OCR text" text={"Invoice 0842\nTotal: 86.40"}
      expanded={expanded} onToggle={() => setExpanded(value => !value)} />}
  </HistoryCard></div>;
}

export function HistoryRichTextFixture({ state }: { state: string }) {
  const compact = state === "compact" || state === "code-compact";
  const isCode = state.startsWith("code");
  const fonts = state === "code-custom" ? {
    "--font-main": '"Georgia", serif',
    "--font-mono": '"Courier New", monospace',
  } as React.CSSProperties : undefined;
  return <HistoryCard compact={compact} style={fonts} disableLayout>
    <HistoryCardMeta source="Editor" time="12:30" actions={<>
      <HistoryCardAction title="Pin"><Pin size={12} /></HistoryCardAction>
      <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
      <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
    </>} />
    <HistoryCardContent rich>
      <HistoryRichTextPreview compact={compact}>
        <div className="rich-text-preview">
          {state === "code-inline" ? <p><strong>Release checklist</strong> <code>pnpm run build</code></p>
            : isCode ? <pre>{"Release checklist\n\nverify preview packages"}</pre>
            : <>
              <p><strong>Release checklist</strong> <em>from the editor</em></p>
              <p><u>Review notes</u> and <a href="#details" tabIndex={-1}>open the draft.</a></p>
              <p>Check the Windows and Linux packages.</p>
              <p>Ask for feedback after the preview.</p>
              <p>Keep the original file with the notes.</p>
            </>}
        </div>
      </HistoryRichTextPreview>
    </HistoryCardContent>
  </HistoryCard>;
}

export function HistoryImageFixture({ state }: { state: string }) {
  const compact = state.startsWith("compact");
  const pending = state.endsWith("pending");
  const missing = state.endsWith("missing");
  const unavailable = state.endsWith("unavailable");
  return <div style={{ width: 220, display: "flex", flexDirection: "column", gap: 4 }}>
    {historyPictures.slice(0, pending || missing || unavailable ? 1 : undefined).map((src, index) =>
      <HistoryCard key={src} compact={compact} disableLayout data-media-measure={historyPictureLabels[index]}>
        <HistoryCardMeta source="Editor" time="12:30" actions={<>
          <HistoryCardAction title="Pin"><Pin size={12} /></HistoryCardAction>
          <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
          <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
        </>} />
        <HistoryCardContent>
          <HistoryImagePreview alt={historyPictureLabels[index]} pending={pending}
            missing={missing ? "Image deleted" : undefined} unavailable={unavailable ? "Preview unavailable" : undefined}
            source={missing || unavailable ? undefined : src} />
        </HistoryCardContent>
      </HistoryCard>)}
  </div>;
}

export function ResizeFixture() {
  const [direction, setDirection] = useState("");
  return <div className="showcase-input-region" data-input-target="resize">
    <span>Resize preview</span>
    <span>{direction}</span>
    <PreviewResizeHandles label="Resize preview" onResize={setDirection} />
  </div>;
}

export function WheelFixture() {
  const [count, setCount] = useState(0);
  return <div className="showcase-input-region" data-input-target="wheel">
    <WheelHotZone onWheel={() => setCount(previous => previous + 1)}>
      <span>Wheel input</span>
      <span>{count ? count + " scroll" : "Scroll over this area"}</span>
    </WheelHotZone>
  </div>;
}
