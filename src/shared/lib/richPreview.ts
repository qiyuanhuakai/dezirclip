import { toTauriLocalImageSrc } from "./localImageSrc";

const RICH_IMAGE_FALLBACK_PREFIX = "<!--TIEZ_RICH_IMAGE:";
const RICH_IMAGE_FALLBACK_SUFFIX = "-->";

export type RichImageFallback = {
  cleanHtml?: string;
  imagePayload?: string;
};

export const extractRichImageFallback = (html?: string): RichImageFallback => {
  if (!html) return {};
  const start = html.lastIndexOf(RICH_IMAGE_FALLBACK_PREFIX);
  if (start < 0) return { cleanHtml: html };

  const markerStart = start + RICH_IMAGE_FALLBACK_PREFIX.length;
  const endRel = html.slice(markerStart).indexOf(RICH_IMAGE_FALLBACK_SUFFIX);
  if (endRel < 0) return { cleanHtml: html };

  const markerEnd = markerStart + endRel;
  const payload = html.slice(markerStart, markerEnd).trim();
  const cleanHtml = `${html.slice(0, start)}${html.slice(markerEnd + RICH_IMAGE_FALLBACK_SUFFIX.length)}`.trim();

  return {
    cleanHtml: cleanHtml || html,
    imagePayload: payload || undefined
  };
};

export const resolveRichImageSrc = (payload?: string): string | null => {
  if (!payload) return null;
  const value = payload.trim();
  if (!value) return null;
  if (value.startsWith("data:image/")) return value;
  if (/^https?:\/\/asset\.localhost\//i.test(value)) return value;
  return toTauriLocalImageSrc(value);
};
