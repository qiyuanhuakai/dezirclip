import { parsePrivacySnapshot } from "../lib/privacyRuntime";
import { readResumeAuthority } from "../lib/mainWindowResume";

type Arguments = Record<string, unknown>;

function privacySnapshot(status: { privacy: { expires_at_ms: number | null } }) {
  const stamp = readResumeAuthority(status);
  return parsePrivacySnapshot({ history_revision: stamp.revision, privacy_epoch: stamp.epoch,
    session: { active: stamp.sessionActive, expires_at_ms: status.privacy.expires_at_ms } });
}

const library = <T>(method: string, ...args: unknown[]) => window.mygo.call<T>(`Library.${method}`, ...args);
const commands: Record<string, (args: Arguments) => Promise<unknown>> = {
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
  if (!Object.prototype.hasOwnProperty.call(commands, command)) throw new Error(`Unsupported MyGo desktop command: ${command}`);
  return await commands[command](args) as T;
}
