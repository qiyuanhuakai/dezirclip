export type EmojiTab = "emoji" | "favorites";

export function EmojiTabs({ active, labels, onChange }: {
  active: EmojiTab;
  labels: Record<EmojiTab, string>;
  onChange: (tab: EmojiTab) => void;
}) {
  const tabs: EmojiTab[] = ["emoji", "favorites"];
  return <div className="emoji-tabs">
    {tabs.map(tab => <button key={tab} type="button" aria-pressed={active === tab}
      className={`btn-icon emoji-tab${active === tab ? " active" : ""}`} onClick={() => onChange(tab)}>
      {labels[tab]}
    </button>)}
  </div>;
}

export function EmojiGrid({ groups, empty, onSelect }: {
  groups: readonly { name: string; emojis: readonly string[] }[];
  empty: string;
  onSelect: (emoji: string) => void;
}) {
  return <div className="emoji-content">
    {groups.map(group => <div key={group.name} className="emoji-group">
      <div className="emoji-group-title">{group.name}</div>
      <div className="emoji-grid">
        {group.emojis.map(emoji => <button key={emoji} type="button" className="emoji-btn"
          onClick={() => onSelect(emoji)} title={emoji}>{emoji}</button>)}
      </div>
    </div>)}
    {groups.every(group => !group.emojis.length) && <div className="emoji-empty">{empty}</div>}
  </div>;
}
