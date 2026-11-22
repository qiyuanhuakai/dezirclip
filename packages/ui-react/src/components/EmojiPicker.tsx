import { useEffect, useRef, useState, type ReactNode } from "react";
import { Search } from "lucide-react";

export type EmojiTab = "emoji" | "symbols" | "favorites";

export type CharacterPreview =
  | { type: "sprite"; src: string; index: number; columns: number; rows: number }
  | { type: "font"; family: string }
  | { type: "code"; code: string };

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

export function EmojiGrid({ groups, empty, onSelect, footer, onLoadMore, getPreview }: {
  groups: readonly { name: string; emojis: readonly (string | { value: string; name: string })[] }[];
  empty: string;
  onSelect: (emoji: string) => void;
  footer?: ReactNode;
  onLoadMore?: () => void;
  getPreview?: (value: string) => CharacterPreview | undefined;
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
    {groups.map((group, groupIndex) => {
      const entries = group.emojis.map(entry => {
        const value = typeof entry === "string" ? entry : entry.value;
        return { value, name: typeof entry === "string" ? undefined : entry.name, preview: getPreview?.(value) };
      });
      const showDescriptions = entries.some(entry => entry.preview?.type === "code");
      return <div key={group.name} className="emoji-group">
        <div className="emoji-group-title">{group.name}</div>
        <div className={showDescriptions ? "emoji-grid emoji-grid--descriptions" : "emoji-grid"}>
          {entries.map(({ value, name, preview }, index) => {
            const isLast = groupIndex === lastGroup && index === entries.length - 1;
            const description = preview?.type === "code" ? [name, preview.code].filter(Boolean).join(" · ") : name;
            return <button key={value} ref={onLoadMore && isLast ? setLastTile : undefined}
              type="button" className="emoji-btn" aria-label={value} aria-description={description}
              onClick={() => onSelect(value)} title={description ?? value}>
              <CharacterGlyph value={value} name={name} preview={preview} />
            </button>;
          })}
        </div>
      </div>;
    })}
    {groups.every(group => !group.emojis.length) && <div className="emoji-empty">{empty}</div>}
    {footer}
  </div>;
}

function CharacterGlyph({ value, name, preview }: { value: string; name?: string; preview?: CharacterPreview }) {
  if (preview?.type === "sprite") {
    const { src, index, columns, rows } = preview;
    const x = index % columns / Math.max(1, columns - 1) * 100;
    const y = Math.floor(index / columns) / Math.max(1, rows - 1) * 100;
    return <span className="emoji-glyph-sprite" aria-hidden="true" style={{
      backgroundImage: `url("${src}")`, backgroundSize: `${columns * 100}% ${rows * 100}%`, backgroundPosition: `${x}% ${y}%`,
    }} />;
  }
  if (preview?.type === "code") {
    return <span className="emoji-glyph-description" aria-hidden="true">
      <span>{preview.code}</span>{name && <span className="emoji-glyph-name">{name}</span>}
    </span>;
  }
  return preview?.type === "font" ? <span style={{ fontFamily: preview.family }}>{value}</span> : value;
}
