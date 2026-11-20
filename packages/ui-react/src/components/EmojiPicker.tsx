import type { ReactNode } from "react";
import { Search } from "lucide-react";

export type EmojiTab = "emoji" | "symbols" | "favorites";

export function CharacterSearch({ label, value, onChange }: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return <div className="emoji-search">
    <Search className="search-icon" size={14} aria-hidden="true" />
    <input className="search-input" type="text" role="searchbox" value={value}
      aria-label={label} placeholder={label} onChange={event => onChange(event.currentTarget.value)} />
  </div>;
}

export function EmojiTabs({ active, labels, onChange }: {
  active: EmojiTab;
  labels: Record<"emoji" | "favorites", string> & Partial<Record<"symbols", string>>;
  onChange: (tab: EmojiTab) => void;
}) {
  const tabs: EmojiTab[] = labels.symbols ? ["emoji", "symbols", "favorites"] : ["emoji", "favorites"];
  return <div className="emoji-tabs">
    {tabs.map(tab => <button key={tab} type="button" aria-pressed={active === tab}
      className={`btn-icon emoji-tab${active === tab ? " active" : ""}`} onClick={() => onChange(tab)}>
      {labels[tab]}
    </button>)}
  </div>;
}

export function EmojiGrid({ groups, empty, onSelect, footer }: {
  groups: readonly { name: string; emojis: readonly (string | { value: string; name: string })[] }[];
  empty: string;
  onSelect: (emoji: string) => void;
  footer?: ReactNode;
}) {
  return <div className="emoji-content">
    {groups.map(group => <div key={group.name} className="emoji-group">
      <div className="emoji-group-title">{group.name}</div>
      <div className="emoji-grid">
        {group.emojis.map(entry => {
          const value = typeof entry === "string" ? entry : entry.value;
          const name = typeof entry === "string" ? undefined : entry.name;
          return <button key={value} type="button" className="emoji-btn" aria-label={value} aria-description={name}
            onClick={() => onSelect(value)} title={name ?? value}>{value}</button>;
        })}
      </div>
    </div>)}
    {groups.every(group => !group.emojis.length) && <div className="emoji-empty">{empty}</div>}
    {footer}
  </div>;
}
