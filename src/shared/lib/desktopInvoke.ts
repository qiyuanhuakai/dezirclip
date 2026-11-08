import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { ClipboardEntry } from "../types";

export function derivedCopySource(entry: ClipboardEntry, protectedContent: boolean, epoch: number) {
  const source = { expectedHostInstanceId: entry.host_instance_id, expectedPrivacyEpoch: epoch };
  if (import.meta.env.MODE === "mygo") return { ...source, derivedFromId: entry.id, allowSensitive: protectedContent };
  return { ...source, derivedFromId: protectedContent ? entry.id : undefined };
}

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (import.meta.env.MODE === "mygo") {
    const { invokeMyGo } = await import("../mygo/commands");
    return invokeMyGo<T>(command, args);
  }
  return args === undefined ? tauriInvoke<T>(command) : tauriInvoke<T>(command, args);
}
