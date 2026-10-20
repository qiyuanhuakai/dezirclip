import { usePlatformCapabilities } from "../../../../shared/hooks/usePlatformCapabilities";
import "../../../../styles/components/wayland.css";

export default function WaylandCapabilitiesSection({ t }: { t: (key: string) => string }) {
  const { report, failed } = usePlatformCapabilities();
  if (!report?.native_wayland && !failed) return null;
  const backend = report?.wayland?.connection;
  return <section className="wayland-capabilities" role="status">
    <strong>{t(failed ? "platform_capabilities_unavailable" : "wayland_experimental_title")}</strong>
    {report?.native_wayland && <>
      <p>{t("wayland_experimental_scope")}</p>
      <dl>
        <dt>{t("wayland_clipboard_backend")}</dt>
        <dd>{backend ? `${backend.protocol} v${backend.bound_version}` : t("wayland_protocol_unavailable")}</dd>
        <dt>{t("wayland_read_write")}</dt>
        <dd>{t(report.read.availability.status === "available" && report.write.availability.status === "available" ? "wayland_available" : "wayland_unavailable")}</dd>
        <dt>{t("wayland_source_identity")}</dt><dd>{t("wayland_source_unavailable")}</dd>
      </dl>
      <p>{t("wayland_interaction_unavailable")}</p>
      <p>{t("wayland_portal_scope")}</p>
    </>}
  </section>;
}
