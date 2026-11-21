import type { ComponentProps, ReactNode } from "react";
import { Plus, X } from "lucide-react";

interface EmojiFavoritesPanelProps extends ComponentProps<"div"> {
  emptyTitle?: string;
  hint: string;
  status?: string;
  dragging?: boolean;
  dropLabel?: string;
}

export function EmojiFavoritesPanel({ children, className, emptyTitle, hint, status, dragging, dropLabel, ...props }: EmojiFavoritesPanelProps) {
  return <div {...props} className={["emoji-fav-region", className].filter(Boolean).join(" ")}>
    <div className="emoji-content">
      <div className="emoji-fav-grid">{children}</div>
      {!dragging && (emptyTitle && !status ? <div className="emoji-fav-empty">
          <div className="emoji-empty-title">{emptyTitle}</div>
          <div className="emoji-empty-desc">{hint}</div>
        </div>
          : <div className="emoji-fav-tip" role={status ? "status" : undefined}>{status || hint}</div>)}
    </div>
    {dragging && dropLabel && <div className="emoji-fav-drop-hint" role="status">{dropLabel}</div>}
  </div>;
}

export function EmojiAddTile({ label, disabled, onAdd }: { label: string; disabled?: boolean; onAdd: () => void }) {
  return <div className="emoji-fav-card emoji-fav-add">
    <button type="button" className="emoji-fav-preview emoji-fav-add-btn" title={label} disabled={disabled} onClick={event => {
      event.stopPropagation();
      onAdd();
    }}>
      <Plus size={18} /><span>{label}</span>
    </button>
  </div>;
}

export function EmojiFavoriteTile({ name, removeLabel, removeVisible, onRevealRemove, onRemove, onChoose, children }: {
  name: string;
  removeLabel: string;
  removeVisible?: boolean;
  onRevealRemove: () => void;
  onRemove: () => void;
  onChoose: () => void;
  children: ReactNode;
}) {
  return <div className="emoji-fav-card" data-delete-visible={removeVisible} onContextMenu={event => {
    event.preventDefault();
    event.stopPropagation();
    onRevealRemove();
  }}>
    <button type="button" className="btn-icon emoji-fav-remove" title={removeLabel} onClick={event => {
      event.stopPropagation();
      onRemove();
    }}><X size={12} /></button>
    <button type="button" className="emoji-fav-preview" title={name} onClick={onChoose}>{children}</button>
  </div>;
}
