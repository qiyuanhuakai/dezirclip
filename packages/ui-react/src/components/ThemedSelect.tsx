import { useLayoutEffect, useMemo, useRef, useState } from "react";
import Select from "react-select";
import type { FilterOptionOption, SelectInstance, SingleValue } from "react-select";
import { FuzzyIndex } from "../fuzzy";

export interface ThemedSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  fontFamily?: string;
}

export interface ThemedSelectProps {
  options: ThemedSelectOption[];
  value: string;
  onChange: (value: string) => void | Promise<void>;
  width?: string;
  placeholder?: string;
  searchable?: boolean;
  noOptionsMessage?: string;
  isDisabled?: boolean;
  inputId?: string;
  ariaLabel?: string;
  onActivate?: () => void;
}

const ThemedSelect = ({
  options,
  value,
  onChange,
  width = "160px",
  placeholder,
  searchable = false,
  noOptionsMessage,
  isDisabled = false,
  inputId,
  ariaLabel,
  onActivate,
}: ThemedSelectProps) => {
  const selected = options.find((option) => option.value === value) ?? null;
  const control = useRef<SelectInstance<ThemedSelectOption, false>>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  useLayoutEffect(() => {
    if (!menuOpen) return;
    const frame = requestAnimationFrame(() => {
      if (control.current?.inputRef?.value) return;
      const list = control.current?.menuListRef;
      const choice = list?.querySelector('[role="option"][aria-selected="true"]');
      if (!list || !choice) return;
      const row = choice.getBoundingClientRect(), viewport = list.getBoundingClientRect();
      if (row.top < viewport.top) list.scrollTop += row.top - viewport.top;
      else if (row.bottom > viewport.bottom) list.scrollTop += row.bottom - viewport.bottom;
    });
    return () => cancelAnimationFrame(frame);
  }, [menuOpen]);

  const index = useMemo(
    () =>
      new FuzzyIndex(options, {
        keys: [
          { name: "label", weight: 2 },
          { name: "value", weight: 1 }
        ],
        threshold: 0.4,
        ignoreDiacritics: true,
        minMatchCharLength: 1
      }),
    [options]
  );

  const allowCacheRef = useRef<{ input: string; index: FuzzyIndex<ThemedSelectOption>; values: Set<string> } | null>(null);

  const filterOption = useMemo(
    () =>
      searchable
        ? (option: FilterOptionOption<ThemedSelectOption>, rawInput: string) => {
            const input = rawInput ?? "";
            if (!input) return true;
            const cached = allowCacheRef.current;
            if (cached?.index === index && cached.input === input) return cached.values.has(option.value);
            const values = new Set(index.search(input, options.length).map(match => match.item.value));
            allowCacheRef.current = { input, index, values };
            return values.has(option.value);
          }
        : undefined,
    [searchable, index, options]
  );

  return (
    <div
      className="settings-themed-select"
      style={{ width }}
      onMouseDown={onActivate}
      onFocus={onActivate}
    >
      <Select
        ref={control}
        aria-label={ariaLabel}
        inputId={inputId}
        classNamePrefix="dezirclip-select"
        options={options}
        value={selected}
        placeholder={placeholder}
        isSearchable={searchable}
        isClearable={false}
        isDisabled={isDisabled}
        isOptionDisabled={(option) => !!option.disabled}
        noOptionsMessage={() => noOptionsMessage ?? "无匹配项"}
        menuPortalTarget={document.body}
        menuPosition="fixed"
        filterOption={filterOption}
        onMenuOpen={() => setMenuOpen(true)}
        onMenuClose={() => setMenuOpen(false)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.nativeEvent.isComposing && !isDisabled && !menuOpen) {
            event.preventDefault();
            control.current?.openMenu("first");
          }
        }}
        onChange={(option: SingleValue<ThemedSelectOption>) => {
          if (!option) return;
          void onChange(option.value);
        }}
        styles={{
          control: (base, state) => ({
            ...base,
            minHeight: "34px",
            width: "100%",
            borderRadius: "var(--select-control-radius)",
            border: state.isFocused ? "var(--select-control-focus-border)" : "var(--select-control-border)",
            background: "var(--select-control-bg)",
            boxShadow: state.isFocused ? "var(--select-control-focus-shadow)" : "var(--select-control-shadow)",
            cursor: "pointer",
            fontSize: "12px"
          }),
          input: (base) => ({
            ...base,
            color: "var(--select-single-value-color)",
            caretColor: "var(--accent-color)"
          }),
          singleValue: (base, { data }) => ({
            ...base,
            ...(data.fontFamily ? { fontFamily: data.fontFamily } : {}),
            color: "var(--select-single-value-color)",
            fontWeight: 600
          }),
          placeholder: (base) => ({
            ...base,
            color: "var(--select-placeholder-color)",
            fontWeight: 500
          }),
          dropdownIndicator: (base) => ({
            ...base,
            color: "var(--select-indicator-color)",
            padding: "0 8px"
          }),
          indicatorSeparator: () => ({
            display: "none"
          }),
          menuPortal: (base) => ({
            ...base,
            zIndex: 99999
          }),
          menu: (base) => ({
            ...base,
            marginTop: "4px",
            borderRadius: "10px",
            overflow: "hidden",
            border: "var(--select-menu-border)",
            background: "var(--select-menu-bg)",
            boxShadow: "var(--select-menu-shadow)"
          }),
          option: (base, state) => ({
            ...base,
            ...(state.data.fontFamily ? { fontFamily: state.data.fontFamily } : {}),
            fontSize: "12px",
            overflowWrap: "anywhere",
            cursor: "pointer",
            background: state.isSelected
              ? "var(--select-option-selected-bg)"
              : state.isFocused
                ? "var(--select-option-focus-bg)"
                : "transparent",
            color: state.isSelected
              ? "var(--select-option-selected-color)"
              : state.isFocused
                ? "var(--select-option-focus-color)"
                : "var(--select-option-color)"
          })
        }}
      />
    </div>
  );
};

export default ThemedSelect;
