import { invoke } from "./desktopInvoke";
import { beginPrivacyAccess, invokePrivacyBound, subscribePrivacyInvalidation } from "./privacySession";
import type { ClipboardEntry } from "../types";

const sourceAppIconCache = new Map<string, string | null>();
const sourceAppIconRequests = new Map<string, Promise<string | null>>();
const SOURCE_APP_ICON_CACHE_LIMIT = 256;

const normalizeSourceAppPath = (sourceAppPath?: string | null) => {
  const value = sourceAppPath?.trim();
  if (!value) return "";
  return value.replace(/\//g, "\\").toLowerCase();
};

export const peekSourceAppIcon = (sourceAppPath?: string | null) => {
  const cacheKey = normalizeSourceAppPath(sourceAppPath);
  if (!cacheKey) return undefined;
  return sourceAppIconCache.get(cacheKey);
};

export const getSourceAppIcon = async (sourceAppPath?: string | null): Promise<string | null> => {
  const rawPath = sourceAppPath?.trim();
  const cacheKey = normalizeSourceAppPath(rawPath);
  if (!rawPath || !cacheKey) return null;

  const cached = sourceAppIconCache.get(cacheKey);
  if (cached !== undefined) {
    sourceAppIconCache.delete(cacheKey);
    sourceAppIconCache.set(cacheKey, cached);
    return cached;
  }

  const pending = sourceAppIconRequests.get(cacheKey);
  if (pending) {
    return pending;
  }

  const request = invoke<string | null>("get_executable_icon", { executablePath: rawPath })
    .then((icon) => {
      const normalizedIcon = typeof icon === "string" && icon.trim() ? icon : null;
      sourceAppIconCache.set(cacheKey, normalizedIcon);
      if (sourceAppIconCache.size > SOURCE_APP_ICON_CACHE_LIMIT) {
        const oldestKey = sourceAppIconCache.keys().next().value;
        if (oldestKey) {
          sourceAppIconCache.delete(oldestKey);
        }
      }
      return normalizedIcon;
    })
    .catch((error) => {
      console.error("Failed to load source app icon:", error);
      return null;
    })
    .finally(() => {
      sourceAppIconRequests.delete(cacheKey);
    });

  sourceAppIconRequests.set(cacheKey, request);
  return request;
};


// The Host keeps executable paths private. A row identity is sufficient to request its icon.
const historyIcons = new Map<string, Promise<string | null>>();
let historyIconQueue: Promise<unknown> = Promise.resolve();
subscribePrivacyInvalidation(() => historyIcons.clear());

export function getHistorySourceAppIcon(entry: Pick<ClipboardEntry, "id" | "host_instance_id" | "timestamp">): Promise<string | null> {
  const access = beginPrivacyAccess("list");
  if (!access || !entry.host_instance_id) return Promise.resolve(null);
  const key = JSON.stringify([entry.host_instance_id, entry.id, entry.timestamp, access.epoch]);
  const cached = historyIcons.get(key);
  if (cached) return cached;
  const request = historyIconQueue.then(async () => {
    if (!access.isCurrent()) return null;
    const icon = await invokePrivacyBound<string | null>("get_source_app_icon", {
      id: entry.id, expectedHostInstanceId: entry.host_instance_id, expectedEpoch: access.epoch
    });
    return access.isCurrent() ? icon : null;
  }).catch(() => {
    if (historyIcons.get(key) === request) historyIcons.delete(key);
    return null;
  });
  historyIconQueue = request;
  historyIcons.set(key, request);
  if (historyIcons.size > SOURCE_APP_ICON_CACHE_LIMIT) {
    const oldest = historyIcons.keys().next().value;
    if (oldest !== undefined) historyIcons.delete(oldest);
  }
  return request;
}
