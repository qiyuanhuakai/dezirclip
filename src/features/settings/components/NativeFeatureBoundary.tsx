import type { ReactNode } from "react";
import { usePlatformCapabilities } from "../../../shared/hooks/usePlatformCapabilities";
import "../../../styles/components/wayland.css";

export default function NativeFeatureBoundary({ t, children }: { t: (key: string) => string; children: ReactNode }) {
  const { desktopInteractions, pending } = usePlatformCapabilities();
  return <>
    {!desktopInteractions && <p className="native-feature-reason">{t(pending ? "platform_capabilities_checking" : "wayland_interaction_unavailable")}</p>}
    <fieldset className="native-feature-boundary" disabled={!desktopInteractions} inert={!desktopInteractions}>
      {children}
    </fieldset>
  </>;
}
