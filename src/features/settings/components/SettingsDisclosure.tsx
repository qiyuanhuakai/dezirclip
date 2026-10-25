import { useId, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

export default function SettingsDisclosure({ title, className = "", children }: {
    readonly title: string;
    readonly className?: string;
    readonly children: ReactNode;
}) {
    const [open, setOpen] = useState(false);
    const contentId = useId();
    return <div className={`settings-disclosure ${className}`}>
        <button type="button" className="settings-disclosure-toggle" aria-expanded={open} aria-controls={contentId}
            onClick={() => setOpen(value => !value)}>
            <span className="settings-disclosure-chevron" aria-hidden="true">{open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</span>
            <span>{title}</span>
        </button>
        <div id={contentId} hidden={!open}>{children}</div>
    </div>;
}
