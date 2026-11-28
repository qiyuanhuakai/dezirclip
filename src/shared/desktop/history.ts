import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { DesktopPageParams, DesktopTagPageParams } from "../protocol/wire";
import type { ClipboardEntry } from "../types";
import type {} from "../mygo/bridge";

type ListAccess = { readonly epoch: number; isCurrent(): boolean };

type PageRequest = Pick<DesktopPageParams, "limit" | "offset"> & { kind?: string | null };
type TagPageRequest = Pick<DesktopTagPageParams, "tag" | "query" | "limit" | "offset"> & {
  sortBy: DesktopTagPageParams["sort_by"];
};

export interface TagItemsPage {
  items: ClipboardEntry[];
  total: number;
  has_more: boolean;
  next_offset: number | null;
}

async function readList<T>(access: ListAccess | null, load: (epoch: number) => Promise<T>): Promise<T> {
  if (!access) throw new Error("PrivacyStateUnavailable");
  if (!access.isCurrent()) throw new Error("PrivacyEpochChanged");
  const result = await load(access.epoch);
  if (!access.isCurrent()) throw new Error("PrivacyEpochChanged");
  return result;
}

export const desktopHistory = {
  page(access: ListAccess | null, { limit, offset, kind }: PageRequest) {
    return readList(access, epoch => import.meta.env.MODE === "mygo"
      ? window.mygo.call<ClipboardEntry[]>("Library.Page", { limit, offset, kind: kind ?? undefined } satisfies DesktopPageParams, epoch)
      : tauriInvoke<ClipboardEntry[]>("get_clipboard_history", { limit, offset, contentType: kind }));
  },
  search(access: ListAccess | null, term: string, limit: number) {
    return readList(access, epoch => import.meta.env.MODE === "mygo"
      ? window.mygo.call<ClipboardEntry[]>("Library.Page", { limit, offset: 0, query: term } satisfies DesktopPageParams, epoch)
      : tauriInvoke<ClipboardEntry[]>("search_clipboard_history", { searchTerm: term, limit }));
  },
  tagPage(access: ListAccess | null, { tag, query, sortBy, limit, offset }: TagPageRequest) {
    return readList(access, epoch => import.meta.env.MODE === "mygo"
      ? window.mygo.call<TagItemsPage>("Library.TagPage", {
        tag, query, sort_by: sortBy, limit, offset, expected_epoch: epoch,
      } satisfies Omit<DesktopTagPageParams, "service_instance_id">)
      : tauriInvoke<TagItemsPage>("get_tag_items_page", { tag, query, sortBy, limit, offset, expectedEpoch: epoch }));
  },
  tagCounts(access: ListAccess | null) {
    return readList(access, epoch => import.meta.env.MODE === "mygo"
      ? window.mygo.call<Record<string, number>>("Library.TagCounts", epoch)
      : tauriInvoke<Record<string, number>>("get_all_tags_info"));
  },
};
