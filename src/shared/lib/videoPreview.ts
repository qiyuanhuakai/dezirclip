export const seekVideoPreviewFrame = (video: HTMLVideoElement | null) => {
  if (!video) return;
  const duration = video.duration;
  if (!Number.isFinite(duration) || duration <= 0) return;
  const maxSeek = Math.max(duration - 0.05, 0);
  if (maxSeek <= 0) return;
  const preferred = Math.min(duration * 0.1, 2);
  const target = Math.min(Math.max(preferred, 0.1), maxSeek);
  if (target <= 0) return;
  try {
    video.currentTime = target;
  } catch {
    // Ignore seek errors; fallback will just show the first frame.
  }
};
