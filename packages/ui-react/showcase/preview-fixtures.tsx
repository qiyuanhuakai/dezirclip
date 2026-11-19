import { useState } from "react";
import { AttachmentPreview, HistoryCard, PreviewImage, PreviewResizeHandles, PreviewToolbar, WheelHotZone,
  type PreviewZoom } from "../src";

const filenameList = ["Project.png", "Screenshot.png", "Notes.pdf", "Planning.xlsx", "Desktop.zip", "Summary.txt", "Log.txt"];
function sampleImage(): string {
  const canvas = document.createElement("canvas");
  canvas.width = 160;
  canvas.height = 96;
  const context = canvas.getContext("2d")!;
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
  return <PreviewToolbar pinned={pinned} opacity={opacity} image zoom={zoom} labels={labels}
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
  const compact = state === "compact";
  const expanded = state === "expanded";
  const markup = state === "media" || expanded ? filenameList.map((name, index) =>
    `<figure>${index < 2 ? `<img src="${picture}" alt="${name}">` : ""}<figcaption>${name}</figcaption></figure>`).join("") : null;
  return <HistoryCard compact={compact} disableLayout>
    <div className="content-preview">
      <AttachmentPreview names={filenameList.join("\n")} html={markup}
        fileLabel="Attachments" expanded={expanded} compact={compact} />
    </div>
  </HistoryCard>;
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
