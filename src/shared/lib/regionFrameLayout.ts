export interface RegionFrameLayout {
  frameWidth: number;
  frameHeight: number;
  scale: number;
  viewportWidth: number;
  viewportHeight: number;
}

/** Integer CSS viewport dimensions can round either side of frame pixels / scale. */
export const matchesRegionFrameLayout = ({
  frameWidth,
  frameHeight,
  scale,
  viewportWidth,
  viewportHeight,
}: RegionFrameLayout): boolean => {
  if (!Number.isSafeInteger(frameWidth) || frameWidth <= 0 ||
      !Number.isSafeInteger(frameHeight) || frameHeight <= 0 ||
      !Number.isFinite(scale) || scale <= 0 ||
      !Number.isFinite(viewportWidth) || viewportWidth <= 0 ||
      !Number.isFinite(viewportHeight) || viewportHeight <= 0) return false;

  return Math.abs(viewportWidth - frameWidth / scale) < 1 &&
    Math.abs(viewportHeight - frameHeight / scale) < 1;
};
