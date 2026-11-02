import type { ClipboardEntry } from "../types";
import { concealSensitiveEntry, isSensitiveEntry } from "./privacyRuntime";

export interface PageStamp {
  instanceId: string;
  epoch: number;
  revision: number;
  sessionActive: boolean;
}

// A standard page has 120 rows, each with up to 2,000 body and 500 preview characters.
const MAX_PAGE_UNITS = 384 * 1024;

/** One bounded ordinary first-page response, never hydrated card details or session history. */
export function createResumePage() {
  let page: { stamp: PageStamp; key: string; rows: ClipboardEntry[] } | null = null;
  return {
    clear() { page = null; },
    save(stamp: PageStamp, key: string, rows: ClipboardEntry[]) {
      page = null;
      if (stamp.sessionActive || !rows.length || rows.length > 160) return;
      let units = 0;
      for (const row of rows) {
        if (row.id <= 0 || row.host_instance_id !== stamp.instanceId) return;
        if (isSensitiveEntry(row) && (row.content || row.html_content || row.ocr_text || row.source_app_path)) return;
        for (const field of Object.values(row)) {
          if (typeof field === "string") units += field.length;
          else if (Array.isArray(field)) {
            units += field.length;
            for (const value of field) if (typeof value === "string") units += value.length;
          }
        }
        if (units > MAX_PAGE_UNITS) return;
      }
      page = { stamp, key, rows: structuredClone(rows.map(row => isSensitiveEntry(row) ? concealSensitiveEntry(row) : row)) };
    },
    read(stamp: PageStamp, key: string) {
      if (!page || page.key !== key || stamp.sessionActive || page.stamp.instanceId !== stamp.instanceId
        || page.stamp.epoch !== stamp.epoch || page.stamp.revision !== stamp.revision) {
        page = null;
        return null;
      }
      return structuredClone(page.rows);
    }
  };
}
