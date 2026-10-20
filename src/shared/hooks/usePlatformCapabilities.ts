import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface RuntimeCapability {
  availability: { status: "available" | "unsupported"; reason?: string };
  backend: string;
  probe: string;
  active: boolean;
}
export interface SessionCapabilities {
  native_wayland: boolean;
  read: RuntimeCapability;
  write: RuntimeCapability;
  monitor: RuntimeCapability;
  auto_paste: RuntimeCapability;
  global_shortcut: RuntimeCapability;
  resolve_source: RuntimeCapability;
  read_privacy_hints: RuntimeCapability;
  wayland?: {
    experimental: boolean;
    connection?: { protocol: string; seat_name?: string; advertised_version: number; bound_version: number };
    failure?: string;
    portals: { interface: string; version?: number; status: string; permission: string; adapter_attached: boolean }[];
  };
}

let request: Promise<SessionCapabilities> | null = null;
function loadCapabilities(): Promise<SessionCapabilities> {
  request ??= Promise.resolve(invoke<SessionCapabilities>("get_platform_capabilities")).then(report => {
    if (typeof report?.native_wayland !== "boolean" || !report.read?.availability || !report.write?.availability) {
      throw new Error("PlatformCapabilityProbeFailed");
    }
    return report;
  });
  return request;
}

/** One metadata probe per webview; no clipboard body or permission prompt. */
export function usePlatformCapabilities() {
  const [report, setReport] = useState<SessionCapabilities | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    loadCapabilities().then(value => { if (alive) setReport(value); })
      .catch(() => { if (alive) setFailed(true); });
    return () => { alive = false; };
  }, []);
  return { report, failed, pending: report === null && !failed,
    desktopInteractions: report !== null && !report.native_wayland };
}
