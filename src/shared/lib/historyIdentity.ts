import type { ClipboardEntry } from "../types";

export function entryInstanceArgs(hostInstanceId?: string): { expectedHostInstanceId?: string } {
  return hostInstanceId === undefined ? {} : { expectedHostInstanceId: hostInstanceId };
}

export function entryIdentityArgs(entry: Pick<ClipboardEntry, "id" | "host_instance_id">) {
  return { id: entry.id, ...entryInstanceArgs(entry.host_instance_id) };
}

export function matchesEntryIdentity(entry: ClipboardEntry, id: number, hostInstanceId?: string): boolean {
  return entry.id === id && entry.host_instance_id === hostInstanceId;
}
