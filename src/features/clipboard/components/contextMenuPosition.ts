export type ContextMenuRect = {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
};

export type ContextMenuViewport = {
  readonly width: number;
  readonly height: number;
  readonly scale: number;
};

export type ContextMenuPointInput = {
  readonly clientX: number;
  readonly clientY: number;
  readonly anchorRect: ContextMenuRect;
  readonly viewport: ContextMenuViewport;
  readonly currentWindowLabel?: string;
  readonly isCompactPreviewOnTop?: boolean;
};

export type ContextMenuPoint = {
  readonly x: number;
  readonly y: number;
};

const isInsideViewport = (x: number, y: number, viewport: ContextMenuViewport) =>
  x >= 0 && y >= 0 && x <= viewport.width && y <= viewport.height;

const clampToAnchor = ({ anchorRect, viewport }: ContextMenuPointInput): ContextMenuPoint => ({
  x: Math.min(Math.max(anchorRect.left, 0), viewport.width),
  y: Math.min(Math.max(anchorRect.top, 0), viewport.height),
});

export const resolveContextMenuPoint = (input: ContextMenuPointInput): ContextMenuPoint => {
  if (input.isCompactPreviewOnTop === true) {
    return clampToAnchor(input);
  }
  if (isInsideViewport(input.clientX, input.clientY, input.viewport)) {
    return { x: input.clientX, y: input.clientY };
  }
  return clampToAnchor(input);
};
