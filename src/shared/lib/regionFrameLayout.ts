export interface RegionFrameLayout {
  frameWidth: number;
  frameHeight: number;
  scale: number;
  viewportWidth: number;
  viewportHeight: number;
}

const validFrameGeometry = ({ frameWidth, frameHeight, scale }: RegionFrameLayout): boolean =>
  Number.isSafeInteger(frameWidth) && frameWidth > 0 &&
  Number.isSafeInteger(frameHeight) && frameHeight > 0 &&
  Number.isFinite(scale) && scale > 0;

/** Integer CSS viewport dimensions can round either side of frame pixels / scale. */
export const matchesRegionFrameLayout = (layout: RegionFrameLayout): boolean => {
  const { frameWidth, frameHeight, scale, viewportWidth, viewportHeight } = layout;
  if (!validFrameGeometry(layout) ||
      !Number.isFinite(viewportWidth) || viewportWidth <= 0 ||
      !Number.isFinite(viewportHeight) || viewportHeight <= 0) return false;

  return Math.abs(viewportWidth - frameWidth / scale) < 1 &&
    Math.abs(viewportHeight - frameHeight / scale) < 1;
};

/** An unmapped WebKitGTK window can report 0×0 until its first presentation. */
export const canDecodeRegionFrame = (layout: RegionFrameLayout, presented: boolean): boolean =>
  (!presented && layout.viewportWidth === 0 && layout.viewportHeight === 0 && validFrameGeometry(layout)) ||
  matchesRegionFrameLayout(layout);
