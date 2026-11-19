import { useId, useState, type ReactNode } from "react";
import { CircleHelp } from "lucide-react";

export function SettingHelp({ children, label, helpLabel }: { readonly children: ReactNode; readonly label?: string; readonly helpLabel: string }) {
    const [open, setOpen] = useState(false);
    const id = useId();
    return <span className="setting-help">
        <button type="button" className="setting-help-button" aria-label={`${helpLabel}${label ? `: ${label}` : ""}`}
            aria-expanded={open} aria-controls={id} onClick={() => setOpen(value => !value)}>
            <CircleHelp size={15} aria-hidden="true" />
        </button>
        <span id={id} className="setting-help-text" hidden={!open}>{children}</span>
    </span>;
}
