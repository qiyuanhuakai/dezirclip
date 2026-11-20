import type { ReactNode } from "react";
import { Plus, X } from "lucide-react";

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
