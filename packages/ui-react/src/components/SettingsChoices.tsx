export interface SettingsChoice<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

export function SettingsChoices<T extends string>({ label, value, options, segmented, className = "", disabled, onChange }: {
  label: string;
  value: T;
  options: readonly SettingsChoice<T>[];
  segmented?: boolean;
  className?: string;
  disabled?: boolean;
  onChange: (value: T) => void;
}) {
  return <div className={[(segmented ? "segmented-control" : "settings-choice-cards"), className].filter(Boolean).join(" ")}
    role="group" aria-label={label}>
    {options.map(option => <button type="button" key={option.value} disabled={disabled || option.disabled}
      aria-pressed={option.value === value}
      className={(segmented ? "segment-btn" : "btn-icon settings-choice-card") + (option.value === value ? " active" : "")}
      onClick={() => { if (value !== option.value) onChange(option.value); }}>
      {option.label}
    </button>)}
  </div>;
}
