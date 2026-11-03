import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (import.meta.env.MODE === "mygo") {
    const { invokeMyGo } = await import("../mygo/commands");
    return invokeMyGo<T>(command, args);
  }
  return tauriInvoke<T>(command, args);
}
