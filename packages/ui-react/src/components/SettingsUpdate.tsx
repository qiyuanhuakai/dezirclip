import { ShieldCheck } from "lucide-react";

export function UpdateVersionRow({ label, version }: { label: string; version?: string }) {
  return <div className="update-version-row">
    <span>{label}</span><strong>{version ? "v" + version : "—"}</strong>
  </div>;
}

export function UpdateProgress({ status, amount, percent, label, verified }: {
  status: string; amount: string; percent: number | null; label: string; verified?: string;
}) {
  return <div className="update-download" role="status" aria-live="polite">
    <div className="settings-range-header">
      <span>{status}</span><span>{amount}</span>
    </div>
    <div className={"update-progress" + (percent === null ? " is-indeterminate" : "")}
      role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent ?? undefined}>
      <span className="update-progress-fill" style={{ transform: `scaleX(${(percent ?? 35) / 100})` }} />
    </div>
    {verified && <p className="update-verified"><ShieldCheck size={14} />{verified}</p>}
  </div>;
}
