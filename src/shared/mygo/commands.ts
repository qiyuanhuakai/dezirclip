import { parsePrivacySnapshot } from "../lib/privacyRuntime";
import { readResumeAuthority } from "../lib/mainWindowResume";
import { hostGeneration } from "./events";
import { getPrivacyViewState } from "../lib/privacySession";
import type { TagBatchSelection } from "../../features/tag/utils/tagBatchSelection";

type Arguments = Record<string, unknown>;

function privacySnapshot(status: { privacy: { expires_at_ms: number | null } }) {
  const stamp = readResumeAuthority(status);
  return parsePrivacySnapshot({ history_revision: stamp.revision, privacy_epoch: stamp.epoch,
    session: { active: stamp.sessionActive, expires_at_ms: status.privacy.expires_at_ms } });
}

const library = <T>(method: string, ...args: unknown[]) => window.mygo.call<T>(`Library.${method}`, ...args);

function currentEpoch() {
  const view = getPrivacyViewState();
  if (view.phase !== "ready" || !view.snapshot || view.pending) throw new Error("PrivacyStateUnavailable");
  return view.snapshot.privacy_epoch;
}

function mutateEntry(args: Arguments, action: string, fields: Arguments = {}) {
  const epoch = currentEpoch();
  if (args.expectedPrivacyEpoch !== undefined && args.expectedPrivacyEpoch !== epoch) throw new Error("PrivacyEpochChanged");
  if ((args.allowSensitive || args.confirmPersistence) && args.expectedPrivacyEpoch === undefined) throw new Error("PrivacyEpochRequired");
  return library<number>("MutateEntry", { action, id: args.id, service_instance_id: args.expectedHostInstanceId,
    expected_epoch: epoch, reveal: args.allowSensitive ?? false, confirm_persistence: args.confirmPersistence, ...fields });
}

const commands: Record<string, (args: Arguments) => Promise<unknown>> = {
  set_window_pinned: async args => {
    await library("SetWindowPinned", args.pinned, currentEpoch());
    return window.mygo.call("DesktopShell.SetAlwaysOnTop", args.pinned);
  },
  delete_clipboard_entry: args => mutateEntry(args, "delete"),
  toggle_clipboard_pin: args => mutateEntry(args, "pin", { pinned: args.isPinned }),
  update_tags: args => mutateEntry(args, "tags", { tags: args.tags }),
  update_item_content: args => mutateEntry(args, "edit", { content: args.newContent }),
  add_manual_item: args => {
    const epoch = currentEpoch();
    if (args.expectedPrivacyEpoch !== epoch) throw new Error("PrivacyEpochChanged");
    return library("AddManual", args.content, { expected_epoch: epoch, content_type: args.contentType,
      tags: args.tags, confirm_persistence: args.confirmPersistence ?? false });
  },
  get_tag_colors: () => library("TagColors", currentEpoch()),
  create_new_tag: args => library("EditTag", "create", args.tagName, "", currentEpoch()),
  remove_tag_from_all: args => library("EditTag", "remove", args.tagName, "", currentEpoch()),
  rename_tag_globally: args => library("EditTag", "rename", args.oldName, args.newName, currentEpoch()),
  clear_clipboard_history: () => library("ClearHistory", currentEpoch()),
  update_pinned_order: args => library("ReorderPins", args.orders, currentEpoch()),
  delete_sensitive_tag_with_contents: args => library("DeleteTagWithContents", args.tagName, currentEpoch()),
  delete_tag_entries: async args => {
    const selection = args.selection as TagBatchSelection;
    if (selection.mode !== "selected" && selection.mode !== "all") throw new Error("InvalidSelection");
    const deletedCount = await library<number>("DeleteTagEntries", { name: args.tagName, expected_epoch: currentEpoch(),
      scope: selection.mode, ids: selection.mode === "selected" ? selection.ids : selection.excludedIds,
      query: selection.mode === "all" ? selection.query : undefined });
    return { deletedCount };
  },
  get_settings: () => library("Preferences"),
  copy_to_clipboard: args => window.mygo.call("History.CopyEntry", {
    id: args.id === 0 ? args.derivedFromId : args.id,
    service_instance_id: args.expectedHostInstanceId,
    expected_epoch: args.expectedPrivacyEpoch ?? args.viewEpoch,
    reveal: args.expectedPrivacyEpoch !== undefined,
    paste: args.paste, with_format: args.pasteWithFormat, image_as_base64: args.pasteImageAsBase64,
    derived_text: args.id === 0 && args.derivedFromId !== undefined ? args.content : undefined,
    move_to_top: args.moveToTop, delete_after_use: args.deleteAfterUse,
  }),
  get_clipboard_host_status: () => library("Status"),
  get_privacy_status: async () => privacySnapshot(await library("Status")),
  set_privacy_session: async args => privacySnapshot(await library("SetPrivacy", {
    active: args.enabled, duration_ms: args.durationMs ?? undefined,
  })),
  get_clipboard_history: args => library("Page", { limit: args.limit, offset: args.offset, kind: args.content_type }, args.expectedEpoch),
  search_clipboard_history: args => library("Page", { limit: args.limit, offset: 0, query: args.searchTerm }, args.expectedEpoch),
  get_all_tags_info: args => library("TagCounts", args.expectedEpoch),
  get_captured_entry_summary: args => library("Summary", { id: args.id, expected_epoch: args.expectedEpoch,
    service_instance_id: args.expectedHostInstanceId }),
  get_tag_items_page: args => library("TagPage", { tag: args.tag, query: args.query ?? "", sort_by: args.sortBy ?? "time",
    limit: args.limit ?? 60, offset: args.offset ?? 0, expected_epoch: args.expectedEpoch }),
  get_privacy_entry: args => library("Entry", { id: args.id, expected_epoch: args.expectedEpoch,
    service_instance_id: args.expectedHostInstanceId, reveal: args.reveal ?? true }),
  get_history_resource_preview: args => library("Entry", { id: args.id, expected_epoch: args.expectedEpoch,
    service_instance_id: args.expectedHostInstanceId, reveal: false,
    thumbnail_dimension: Math.min(1024, Math.max(64, Number(args.maxDimension ?? 384))) }),
};

export async function invokeMyGo<T>(command: string, args: Arguments = {}): Promise<T> {
  if (command === "restore_window_pinned") return window.mygo.call<T>("DesktopShell.SetAlwaysOnTop", args.pinned);
  if (command === "activate_window_focus") return window.mygo.call<T>("DesktopShell.Focus");
  if (command === "hide_window_cmd") return window.mygo.call<T>("mygo:window.Close");
  if (command === "get_platform_capabilities") return library<T>("Platform");
  if (!Object.prototype.hasOwnProperty.call(commands, command)) throw new Error(`Unsupported MyGo desktop command: ${command}`);
  const generation = hostGeneration();
  const result = await commands[command](args);
  if (command !== "set_privacy_session" && command !== "copy_to_clipboard" && hostGeneration() !== generation) throw new Error("PrivacyEpochChanged");
  return result as T;
}
