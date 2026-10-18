let sheet: CSSStyleSheet | null = null;
let sheetText: string | null = null;
let activeCss: string | null = null;
let link: HTMLLinkElement | null = null;
let blobUrl: string | null = null;

const detachSheet = () => {
  if (sheet && "adoptedStyleSheets" in document && document.adoptedStyleSheets.includes(sheet)) {
    document.adoptedStyleSheets = document.adoptedStyleSheets.filter(item => item !== sheet);
  }
};

const clearLink = () => {
  link?.remove();
  link = null;
  if (blobUrl) URL.revokeObjectURL(blobUrl);
  blobUrl = null;
};

/** The caller supplies CSS already checked by the shared package validator. */
export const applyImportedThemeCss = (css: string | null): void => {
  if (css === null) {
    detachSheet();
    clearLink();
    activeCss = null;
    return;
  }
  if (activeCss === css && (link?.isConnected || (sheet && document.adoptedStyleSheets?.includes(sheet)))) return;
  if ("adoptedStyleSheets" in document && typeof CSSStyleSheet !== "undefined" && typeof CSSStyleSheet.prototype.replaceSync === "function") {
    try {
      sheet ??= new CSSStyleSheet();
      if (sheetText !== css) { sheet.replaceSync(css); sheetText = css; }
      if (!document.adoptedStyleSheets.includes(sheet)) document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
      clearLink();
      activeCss = css;
      return;
    } catch (error) {
      detachSheet();
      console.warn("[theme] Constructed stylesheet unavailable", error);
    }
  }
  clearLink();
  const next = document.createElement("link");
  blobUrl = URL.createObjectURL(new Blob([css], { type: "text/css" }));
  next.rel = "stylesheet";
  next.href = blobUrl;
  next.dataset.dezirclipTheme = "custom";
  next.onerror = () => console.error("[theme] Failed to load local theme stylesheet");
  document.head.appendChild(next);
  link = next;
  activeCss = css;
};
