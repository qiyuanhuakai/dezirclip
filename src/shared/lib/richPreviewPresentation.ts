import { hasHistoryPreviewText } from "./historyPreview";
import { isRamImageSource } from "./privacyRuntime";

// This document is inspected in memory; only HtmlContent's sanitizer renders
// it. A bitmap offered alongside CF_HTML is a fallback, not the HTML body.
export function richPreviewPresentation(html: string, fallback: string | null): { html: boolean; appendFallback: boolean; containsImages: boolean } {
    const doc = new DOMParser().parseFromString(html, "text/html");
    doc.querySelectorAll("style,script,head").forEach(node => node.remove());
    const text = hasHistoryPreviewText(doc.body.textContent ?? "");
    const image = Array.from(doc.querySelectorAll("img")).some(node => isRamImageSource(node.getAttribute("src") ?? ""));
    const bitmap = fallback !== null && isRamImageSource(fallback);
    return { html: text || image, appendFallback: text && !image && bitmap, containsImages: image || bitmap };
}
