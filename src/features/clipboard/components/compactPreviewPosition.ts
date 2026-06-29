export type CompactPreviewPoint = {
  readonly x: number;
  readonly y: number;
};

export type CompactPreviewRect = {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
};

export type PickPreviewPositionInput = {
  readonly anchorX: number;
  readonly anchorY: number;
  readonly anchorRect?: CompactPreviewRect | null;
  readonly widthPx: number;
  readonly heightPx: number;
  readonly monitorPos: CompactPreviewPoint;
  readonly monitorSize: { readonly width: number; readonly height: number };
  readonly margin: number;
  readonly offset: number;
  readonly avoidRect?: CompactPreviewRect | null;
};

export const pickPreviewPosition = ({
  anchorX,
  anchorY,
  anchorRect,
  widthPx,
  heightPx,
  monitorPos,
  monitorSize,
  margin,
  offset,
  avoidRect,
}: PickPreviewPositionInput): CompactPreviewPoint => {
  const left = monitorPos.x + margin;
  const top = monitorPos.y + margin;
  const right = monitorPos.x + monitorSize.width - margin;
  const bottom = monitorPos.y + monitorSize.height - margin;
  const upwardY = anchorRect ? anchorRect.bottom - heightPx : anchorY - heightPx - offset;

  const clampPoint = (p: CompactPreviewPoint): CompactPreviewPoint => ({
    x: Math.min(Math.max(p.x, left), right - widthPx),
    y: Math.min(Math.max(p.y, top), bottom - heightPx),
  });

  const intersectsAvoidRect = (p: CompactPreviewPoint) => {
    if (!avoidRect) return false;
    const previewRect = {
      left: p.x,
      top: p.y,
      right: p.x + widthPx,
      bottom: p.y + heightPx,
    };
    return !(
      previewRect.right <= avoidRect.left ||
      previewRect.left >= avoidRect.right ||
      previewRect.bottom <= avoidRect.top ||
      previewRect.top >= avoidRect.bottom
    );
  };

  const candidates = [
    { x: anchorX + offset, y: anchorY + offset },
    { x: anchorX + offset, y: upwardY },
    { x: anchorX - widthPx - offset, y: anchorY + offset },
    { x: anchorX - widthPx - offset, y: upwardY },
  ];

  const fits = (p: CompactPreviewPoint) =>
    p.x >= left && p.y >= top && p.x + widthPx <= right && p.y + heightPx <= bottom;

  for (const c of candidates) {
    if (fits(c) && !intersectsAvoidRect(c)) return c;
  }

  if (avoidRect) {
    const sideY = anchorRect ? anchorRect.bottom - heightPx : anchorY - Math.round(heightPx * 0.25);
    const outsideCandidates = [
      { x: avoidRect.right + offset, y: sideY },
      { x: avoidRect.left - widthPx - offset, y: sideY },
      { x: anchorX - Math.round(widthPx * 0.2), y: avoidRect.top - heightPx - offset },
      { x: anchorX - Math.round(widthPx * 0.2), y: avoidRect.bottom + offset },
    ].map(clampPoint);

    for (const c of outsideCandidates) {
      if (!intersectsAvoidRect(c)) return c;
    }
  }

  for (const c of candidates) {
    const clamped = clampPoint(c);
    if (!intersectsAvoidRect(clamped)) return clamped;
  }

  return clampPoint(candidates[0]);
};
