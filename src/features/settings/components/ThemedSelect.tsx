import { useMemo, useRef } from "react";
import Select from "react-select";
import type { FilterOptionOption, SingleValue } from "react-select";
import { FuzzyIndex } from "../../../shared/lib/fuzzy";

export interface ThemedSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

interface ThemedSelectProps {
  options: ThemedSelectOption[];
  value: string;
  onChange: (value: string) => void | Promise<void>;
  width?: string;
  placeholder?: string;
  searchable?: boolean;
  noOptionsMessage?: string;
  isDisabled?: boolean;
}

const ThemedSelect = ({
  options,
  value,
  onChange,
  width = "160px",
  placeholder,
  searchable = false,
  noOptionsMessage,
  isDisabled = false
}: ThemedSelectProps) => {
  const selected = options.find((option) => option.value === value) ?? null;

  const index = useMemo(
    () =>
      new FuzzyIndex(options, {
        keys: [
          { name: "label", weight: 2 },
          { name: "value", weight: 1 }
        ],
        threshold: 0.4,
        minMatchCharLength: 1
      }),
    [options]
  );

  const allowCacheRef = useRef<{ input: string; values: Set<string> | null }>({
    input: "",
    values: null
  });

  const filterOption = useMemo(
    () =>
      searchable
        ? (option: FilterOptionOption<ThemedSelectOption>, rawInput: string) => {
            const input = rawInput ?? "";
            if (!input) return true;
            const cached = allowCacheRef.current;
            if (cached.input !== input) {
              const matches = index.search(input, options.length);
              cached.input = input;
              cached.values = new Set(
                matches.map((m) => (m.item as ThemedSelectOption).value)
              );
            }
            return cached.values!.has(option.value);
          }
        : undefined,
    [searchable, index, options]
  );

  return (
    <div style={{ width }}>
      <Select
        classNamePrefix="tiez-select"
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
          singleValue: (base) => ({
            ...base,
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
            fontSize: "12px",
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
