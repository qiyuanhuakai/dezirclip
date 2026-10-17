import { extractRichImageFallback, resolveRichImageSrc } from "./richPreview";

// Whether a snapshot is rendered instead of this HTML is decided by each
// caller, not here: the list and the hover preview have different fallback
// state, and both already hold the flag themselves. An earlier version took
// `richTextSnapshotPreview` as a parameter and never read it, which read as
// though passing it did something.
type RichPreviewInput = {
  contentType: string;
  htmlContent?: string;
};

export type RichPreviewData = {
  cleanHtml: string;
  imageSrc: string | null;
};

const EMPTY_RICH_PREVIEW_DATA: RichPreviewData = { cleanHtml: "", imageSrc: null };

export const getRichPreviewData = (payload: RichPreviewInput | null): RichPreviewData => {
  const htmlContent = payload?.contentType === "rich_text" ? payload.htmlContent : undefined;
  if (!htmlContent) return EMPTY_RICH_PREVIEW_DATA;

  const { cleanHtml, imagePayload } = extractRichImageFallback(htmlContent);
  return {
    cleanHtml: cleanHtml ? cleanHtml : htmlContent,
    imageSrc: resolveRichImageSrc(imagePayload)
  };
};
