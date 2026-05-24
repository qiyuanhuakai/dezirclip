import { extractRichImageFallback, resolveRichImageSrc } from "./richPreview";

type RichPreviewInput = {
  contentType: string;
  htmlContent?: string;
  richTextSnapshotPreview?: boolean;
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
