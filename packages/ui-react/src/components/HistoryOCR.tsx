import type { ReactNode } from "react";

export type HistoryOCRStatus = "processing" | "done" | "failed" | "unsupported";

export function HistoryOCRBadge({ status, children }: { status: HistoryOCRStatus; children: ReactNode }) {
  return <span className="ocr-badge" data-testid="ocr-badge" data-status={status}>{children}</span>;
}

export interface HistoryOCRResultProps {
  label: string;
  toggleLabel: string;
  text: string;
  expanded: boolean;
  protectedContent?: boolean;
  onToggle: () => void;
}

export function HistoryOCRResult({ label, toggleLabel, text, expanded, protectedContent, onToggle }: HistoryOCRResultProps) {
  return <div className="ocr-text-section" data-testid="ocr-section"
    data-privacy-protected={protectedContent ? "true" : undefined}
    onDragStart={event => { if (protectedContent) event.preventDefault(); }}>
    <button type="button" className="btn-icon ocr-text-toggle" data-testid="ocr-toggle"
      title={toggleLabel} aria-label={toggleLabel} aria-expanded={expanded} onClick={event => { event.stopPropagation(); onToggle(); }}>
      <span aria-hidden="true">{expanded ? "▼" : "▶"}</span>{label}
    </button>
    {expanded && <div className="ocr-text-content" data-testid="ocr-text" onClick={event => event.stopPropagation()}>{text}</div>}
  </div>;
}
