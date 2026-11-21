import { useEffect, useRef, useState, type ReactNode } from "react";
import { Search } from "lucide-react";

export type EmojiTab = "emoji" | "symbols" | "favorites";

export function CharacterPanel({ children }: { children: ReactNode }) {
  return <div className="emoji-panel">{children}</div>;
}

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

export function EmojiGrid({ groups, empty, onSelect, footer, onLoadMore }: {
  groups: readonly { name: string; emojis: readonly (string | { value: string; name: string })[] }[];
  empty: string;
  onSelect: (emoji: string) => void;
  footer?: ReactNode;
  onLoadMore?: () => void;
}) {
  const viewport = useRef<HTMLDivElement>(null);
  const [lastTile, setLastTile] = useState<HTMLButtonElement | null>(null);
  const lastGroup = groups.reduce((last, group, i) => group.emojis.length ? i : last, -1);
  useEffect(() => {
    if (!onLoadMore || !lastTile || !viewport.current) return;
    let active = true;
    const observer = new IntersectionObserver(entries => {
      if (!active || !entries.some(entry => entry.isIntersecting)) return;
      active = false;
      observer.disconnect();
      onLoadMore();
    }, { root: viewport.current, rootMargin: "0px 0px 120px 0px" });
    observer.observe(lastTile);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [lastTile, onLoadMore]);

  return <div className="emoji-content" ref={viewport}>
    {groups.map((group, groupIndex) => <div key={group.name} className="emoji-group">
      <div className="emoji-group-title">{group.name}</div>
      <div className="emoji-grid">
        {group.emojis.map((entry, index) => {
          const value = typeof entry === "string" ? entry : entry.value;
          const name = typeof entry === "string" ? undefined : entry.name;
          const isLast = groupIndex === lastGroup && index === group.emojis.length - 1;
          return <button key={value} ref={onLoadMore && isLast ? setLastTile : undefined}
            type="button" className="emoji-btn" aria-label={value} aria-description={name}
            onClick={() => onSelect(value)} title={name ?? value}>{value}</button>;
        })}
      </div>
    </div>)}
    {groups.every(group => !group.emojis.length) && <div className="emoji-empty">{empty}</div>}
    {footer}
  </div>;
}
